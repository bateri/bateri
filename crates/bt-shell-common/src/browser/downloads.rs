//! Where a download goes: the name a web page gives a file is the web page's, so it is made safe
//! to write before it is a name in the person's folder, and a file of that name already there is
//! never written over.

use std::path::{Path, PathBuf};

/// The longest name written, bytes (a file name is 255 on the file systems of a Mac).
const NAME_LIMIT: usize = 200;

/// `name` as a file name that is safe to write in a folder: its last part only (no `/`, no `\`),
/// no control character, no leading dot (a hidden file the person did not ask for), not empty,
/// not `.` or `..`, and short. A name that is left with nothing is `download`.
pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '\u{202e}'))
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    if cleaned.is_empty() {
        return "download".to_owned();
    }
    if cleaned.len() <= NAME_LIMIT {
        return cleaned.to_owned();
    }
    // Cut at a character, keeping the extension: a long name with a short extension loses its middle.
    let (stem, extension) = match cleaned.rsplit_once('.') {
        Some((stem, extension)) if extension.len() <= 12 && !stem.is_empty() => {
            (stem, format!(".{extension}"))
        }
        _ => (cleaned, String::new()),
    };
    let mut kept = String::new();
    for c in stem.chars() {
        if kept.len() + c.len_utf8() + extension.len() > NAME_LIMIT {
            break;
        }
        kept.push(c);
    }
    format!("{kept}{extension}")
}

/// The path in `folder` a download called `name` is written to: the safe form of the name, and if
/// a file or folder of that name is there, `name (1).ext`, `name (2).ext` and so on until none is.
pub fn path_in(folder: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let name = safe_name(name);
    let first = folder.join(&name);
    if !exists(&first) {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name.as_str(), String::new()),
    };
    (1..)
        .map(|n| folder.join(format!("{stem} ({n}){extension}")))
        .find(|path| !exists(path))
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_made_safe_to_write() {
        assert_eq!(safe_name("report.pdf"), "report.pdf");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("a\\b\\c.txt"), "c.txt");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("..."), "download");
        assert_eq!(safe_name(""), "download");
        assert_eq!(safe_name("/"), "download");
        assert_eq!(safe_name("tab\there\n.txt"), "tabhere.txt");
        assert_eq!(safe_name("a:b.txt"), "ab.txt");
        // The right-to-left override that makes `gpj.exe` read `exe.jpg`.
        assert_eq!(safe_name("photo\u{202e}gpj.exe"), "photogpj.exe");
    }

    #[test]
    fn a_long_name_keeps_its_extension() {
        let name = format!("{}.tar.gz", "x".repeat(400));
        let safe = safe_name(&name);
        assert!(safe.len() <= NAME_LIMIT);
        assert!(safe.ends_with(".gz"));
        let wide = format!("{}.pdf", "é".repeat(300));
        let safe = safe_name(&wide);
        assert!(safe.len() <= NAME_LIMIT && safe.ends_with(".pdf"));
    }

    #[test]
    fn a_file_that_is_there_is_not_written_over() {
        let folder = Path::new("/d");
        let there = |taken: &'static [&'static str]| {
            move |path: &Path| taken.iter().any(|t| path == folder.join(t))
        };
        assert_eq!(path_in(folder, "a.txt", there(&[])), Path::new("/d/a.txt"));
        assert_eq!(
            path_in(folder, "a.txt", there(&["a.txt"])),
            Path::new("/d/a (1).txt")
        );
        assert_eq!(
            path_in(folder, "a.txt", there(&["a.txt", "a (1).txt"])),
            Path::new("/d/a (2).txt")
        );
        assert_eq!(
            path_in(folder, "Makefile", there(&["Makefile"])),
            Path::new("/d/Makefile (1)")
        );
        assert_eq!(
            path_in(folder, "../x.txt", there(&[])),
            Path::new("/d/x.txt")
        );
    }
}
