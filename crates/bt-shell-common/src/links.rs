//! What a ⌘-clicked link **is** on disk and what opening it does (044 R5).
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
//!   and the shell passes [`stat`]. **A path that does not exist is not a link.**
//! - [`resolve_first`] — a path query's candidates (`My Drive`'s `Drive`, …, `My Drive`) →
//!   the first that exists, iTerm2's semantic history (044 set sonrası).
//! - [`action`] — the policy table (`plan.md` → R5.1): a **white list**, the
//!   [`quote::shell_quote`](crate::quote::shell_quote) precedent — a document of a known
//!   content type without the `x` bit opens in its default application, a directory opens in
//!   Finder, **everything else** is revealed in Finder; an OSC 8 scheme off the common list
//!   asks first; `bateri://` is swallowed on every path. The error's direction is safe: an
//!   unknown file is shown, never run.
//!
//! "Is this a known content type" and "is this directory a package" are AppKit's questions
//! (UTType, `NSWorkspace`), so they come in as an argument ([`Content`]); this module has no
//! queue, no `dispatch2` and no AppKit (`make audit`). The background queue and the return to
//! the main queue are the platform shell's (044 phase-4), the `pane::RemoteProbe` precedent.
//! [`hostname`] reads the machine's name for `SessionOptions::hostname` — `child`'s precedent
//! of a thin system read next to the policy.
//!
//! The rationale is in `.tasks/044-tiklanabilir-baglantilar/discussion.md` → Karar 5 and
//! Muhakeme.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use bt_core::LinkKind;

/// What a file-system object is — the answer of the injected `stat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A directory (a package such as `.app` too: telling it apart is [`Content`]'s job).
    Dir,
    /// Anything else that exists; `executable` is any of the three `x` bits.
    File { executable: bool },
}

/// A path that exists, with what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub path: PathBuf,
    pub entry: Entry,
}

/// The platform's answer about a resolved path's content (UTType on macOS).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Content {
    /// A known document type that is safe to open in its default application: text and
    /// source code, image, PDF, audio/video.
    Document,
    /// A package — a directory the system treats as one file (`.app`, `.pkg`, `.workflow`, …).
    /// Opening it would launch or install something, so it is revealed instead.
    Package,
    /// Anything else.
    Other,
}

/// What a ⌘-click does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// Hand the URL to its default application (`http`, `https`, `ftp`, `mailto`).
    OpenUrl(String),
    /// Open the document in its default application.
    OpenFile(PathBuf),
    /// Show the item selected in Finder — the default for every file off the white list.
    Reveal(PathBuf),
    /// Open the directory in Finder.
    OpenDir(PathBuf),
    /// An OSC 8 URI with an uncommon scheme: ask with the whole target before opening.
    Confirm(String),
    /// `bateri://`: our own scheme is never opened from a click (038 Karar 7).
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
/// link** (iTerm2's semantic history; `discussion.md` → Karar 5).
///
/// - `~` and `~/…` go to `home`; `~user` is not expanded (not a link — it would need a passwd
///   lookup per hover and nothing in the scanner's output asks for it).
/// - An absolute path is taken as is.
/// - A relative path is joined to `cwd`, the pane's **current** OSC 7 directory — not the
///   directory the line was printed in (the named known limit, `plan.md` → Kapsam Dışı).
///
/// The `:line:col` suffix is already off the candidate (`bt-core` splits it into
/// [`LinkKind::Path`]); it is **not** stripped again here, so a file whose name really ends
/// in `:12` still resolves. `stat` follows symlinks ([`stat`]).
pub fn resolve(
    candidate: &Path,
    cwd: Option<&Path>,
    home: Option<&Path>,
    stat: impl Fn(&Path) -> Option<Entry>,
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
    let entry = stat(&path)?;
    Some(Resolved { path, entry })
}

/// A path query's answer (044 set sonrası, iTerm2's semantic history): the
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
    stat: impl Fn(&Path) -> Option<Entry>,
) -> Option<(usize, Resolved)> {
    candidates
        .into_iter()
        .enumerate()
        .find_map(|(index, candidate)| {
            resolve(candidate, cwd, home, &stat).map(|resolved| (index, resolved))
        })
}

/// The production `stat`: `std::fs::metadata` (symlinks followed — a link to a script is a
/// script). Blocking: on a network disk it can hang, which is why the platform shell calls it
/// on a background queue, never on the main thread or in the frame path.
pub fn stat(path: &Path) -> Option<Entry> {
    let meta = std::fs::metadata(path).ok()?;
    Some(if meta.is_dir() {
        Entry::Dir
    } else {
        Entry::File {
            executable: meta.permissions().mode() & 0o111 != 0,
        }
    })
}

/// The policy table (`plan.md` → R5.1). `resolved` is [`resolve`]'s answer for a link that
/// names a local path ([`local_path`]); `content` is asked only for a resolved path.
///
/// | link | action |
/// |---|---|
/// | `bateri:` anything | [`LinkAction::Swallow`] |
/// | local path / `file://`, not found | `None` — not a link |
/// | `file:` without a `//` authority | [`LinkAction::Swallow`] |
/// | directory, not a package | [`LinkAction::OpenDir`] |
/// | package | [`LinkAction::Reveal`] |
/// | file without `x`, known document | [`LinkAction::OpenFile`] |
/// | any other file | [`LinkAction::Reveal`] |
/// | `http`/`https`/`ftp`/`mailto` | [`LinkAction::OpenUrl`] |
/// | OSC 8, any other scheme (or none) | [`LinkAction::Confirm`] |
///
/// The swallow row comes **first** and reads the target whatever the kind: the hit test does
/// not hand `bateri://` out today, the row is the one-line defence 038 Karar 7 asks for.
///
/// A `file:` URL [`local_path`] cannot read (`file:/x`, no authority) is swallowed too:
/// handed to `NSWorkspace` as a URL it would **run** a `.command` or launch an `.app` — the
/// very thing the file rows refuse — and the hit test's remote/authority gate does not see it.
pub fn action(
    target: &str,
    kind: &LinkKind,
    resolved: Option<&Resolved>,
    content: impl FnOnce(&Path) -> Content,
) -> Option<LinkAction> {
    if has_scheme(target, "bateri") {
        return Some(LinkAction::Swallow);
    }
    if local_path(target, kind).is_some() {
        let Resolved { path, entry } = resolved?;
        let path = path.clone();
        return Some(match (entry, content(&path)) {
            (Entry::Dir, Content::Package) => LinkAction::Reveal(path),
            (Entry::Dir, _) => LinkAction::OpenDir(path),
            (Entry::File { executable: false }, Content::Document) => LinkAction::OpenFile(path),
            (Entry::File { .. }, _) => LinkAction::Reveal(path),
        });
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
/// local" question needs it (044 Muhakeme → İşletme 2). `None` if the call fails or the name
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

    /// The fake disk: a script, a document, a directory and a package.
    fn fake(path: &Path) -> Option<Entry> {
        match path.to_str()? {
            "/work/proj/src/main.rs" | "/Users/me/notes.md" | "/work/proj/a:12" => {
                Some(Entry::File { executable: false })
            }
            "/work/proj/run.sh" => Some(Entry::File { executable: true }),
            "/Users/me" | "/work/proj/src" | "/Applications/Foo.app" => Some(Entry::Dir),
            "/tmp/a b" => Some(Entry::File { executable: false }),
            _ => None,
        }
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

    /// The content oracle the macOS shell answers with UTType.
    fn content(path: &Path) -> Content {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("rs" | "md") => Content::Document,
            Some("app") => Content::Package,
            _ => Content::Other,
        }
    }

    fn act(target: &str, kind: LinkKind) -> Option<LinkAction> {
        let resolved = local_path(target, &kind).and_then(|path| res(path.to_str()?));
        action(target, &kind, resolved.as_ref(), content)
    }

    #[test]
    fn relative_tilde_and_absolute_paths_resolve() {
        assert_eq!(
            res("src/main.rs"),
            Some(Resolved {
                path: "/work/proj/src/main.rs".into(),
                entry: Entry::File { executable: false },
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
        let disk = |path: &Path| match path.to_str()? {
            "/work/proj/My Drive" | "/work/proj/Screen Studio Projects" => Some(Entry::Dir),
            "/work/proj/Drive" | "/work/proj/foo.txt" => Some(Entry::File { executable: false }),
            _ => None,
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
        // `bt-core` hands `src/main.rs` with `line: 12, col: 3` — the file opens, the suffix
        // is only recognized (no jump, `plan.md` → Kapsam Dışı).
        let kind = LinkKind::Path {
            line: Some(12),
            col: Some(3),
        };
        assert_eq!(
            act("src/main.rs", kind),
            Some(LinkAction::OpenFile("/work/proj/src/main.rs".into()))
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
        assert_eq!(
            act("file:///tmp/a%20b", LinkKind::Osc8),
            Some(LinkAction::Reveal("/tmp/a b".into()))
        );
        assert_eq!(
            act("file://localhost/work/proj/src", LinkKind::Url),
            Some(LinkAction::OpenDir("/work/proj/src".into()))
        );
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
        // A known document without `x` opens.
        assert_eq!(
            act("src/main.rs", path_kind()),
            Some(LinkAction::OpenFile("/work/proj/src/main.rs".into()))
        );
        // An executable is revealed, never run.
        assert_eq!(
            act("run.sh", path_kind()),
            Some(LinkAction::Reveal("/work/proj/run.sh".into()))
        );
        // An unknown type is revealed (the white list's complement).
        assert_eq!(
            act("/tmp/a b", path_kind()),
            Some(LinkAction::Reveal("/tmp/a b".into()))
        );
        // A directory opens in Finder.
        assert_eq!(
            act("src", path_kind()),
            Some(LinkAction::OpenDir("/work/proj/src".into()))
        );
        // A package is a directory that would launch: revealed.
        assert_eq!(
            act("/Applications/Foo.app", path_kind()),
            Some(LinkAction::Reveal("/Applications/Foo.app".into()))
        );
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

    #[test]
    fn content_is_asked_only_for_a_resolved_path() {
        let ask = |_: &Path| -> Content { panic!("asked for a URL") };
        assert_eq!(
            action("https://x.dev", &LinkKind::Url, None, ask),
            Some(LinkAction::OpenUrl("https://x.dev".into()))
        );
        assert_eq!(action("missing", &path_kind(), None, ask), None);
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
        assert_eq!(stat(&dir), Some(Entry::Dir));
        assert_eq!(stat(&script), Some(Entry::File { executable: true }));
        assert_eq!(stat(&doc), Some(Entry::File { executable: false }));
        assert_eq!(stat(&dir.join("nope")), None);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn this_machine_has_a_name() {
        let name = hostname().expect("gethostname");
        assert!(!name.contains('\0'));
    }
}
