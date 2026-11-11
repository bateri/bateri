//! What a ⌘-click on the path of a file or a folder does: it opens in the program the person
//! chose (`[apps] open_with`), on the computer the path is on, in a floating pane. Where there is
//! no such program, the path is not a link.
//!
//! The decision is pure ([`opens`], in the two steps the click has: [`choose`] before anything is
//! read of the path, [`Choice::takes`] once it is known whether the path is a file or a folder),
//! and so are the words the program is started with ([`argv_here`], [`argv_server`]). The platform
//! shell reads the disk, asks the machine and starts the pane.
//!
//! **The click is a gate that starts a program, so the words are made here and nowhere else.**
//! The program is never found on a `PATH`: it is the machine's link to the program that is
//! followed under that name (`apps/bin/<name>`), and the name is a program's name
//! ([`apps::is_name`]), so it cannot be a path or an option. The path is the one the check found,
//! absolute, and reaches the program as an argument; it is no part of any shell text. On a
//! server, where the home folder is not known here, the one shell line there is — a constant —
//! finds the link by `$HOME` and runs it with the rest as arguments.

use std::path::Path;

use bt_core::pencere::apps::{self, Opens};

use crate::apps::opens::Opener;

/// What was clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thing {
    File,
    Folder,
}

impl Thing {
    fn kind(self) -> Opens {
        match self {
            Thing::File => Opens::File,
            Thing::Folder => Opens::Folder,
        }
    }
}

/// The program a click would open in: the name the setting gives, and what the program says it
/// takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub name: String,
    pub opens: Vec<Opens>,
}

/// The program the setting names, when it is one the person follows that opens files or folders;
/// `None` for `""` (Nothing), a name that is not a program's, and a program that says it opens
/// neither. Known before the path is: what a path that is not there gets is no link, and so is
/// every path with this `None`.
pub fn choose(setting: &str, openers: &[Opener]) -> Option<Choice> {
    if !apps::is_name(setting) {
        return None;
    }
    openers
        .iter()
        .find(|opener| opener.name == setting)
        .map(|opener| Choice {
            name: opener.name.clone(),
            opens: opener.opens.clone(),
        })
}

impl Choice {
    /// Whether the program takes `thing`: a program that says it opens files is not offered a
    /// folder.
    pub fn takes(&self, thing: Thing) -> bool {
        self.opens.contains(&thing.kind())
    }

    /// Whether the machine has the program, by what it said it has (`package state`): `None`, a
    /// machine that has not said, has none that is known.
    pub fn is_on(&self, installed: Option<&[String]>) -> bool {
        installed.is_some_and(|names| names.contains(&self.name))
    }
}

/// The program a click on a `thing` opens in, or `None` when the path is not a link: the whole
/// decision, from the setting, the followed programs that open paths, and what the machine the
/// path is on has installed (`None` before it said).
pub fn opens(
    thing: Thing,
    setting: &str,
    openers: &[Opener],
    installed: Option<&[String]>,
) -> Option<String> {
    choose(setting, openers)
        .filter(|choice| choice.takes(thing) && choice.is_on(installed))
        .map(|choice| choice.name)
}

/// The words after the program's own: `+LINE` when the click was on `path:line` (never a column,
/// never a line that is not one), then the path.
fn arguments(line: Option<u32>, path: &str) -> Vec<String> {
    let mut words = Vec::with_capacity(2);
    if let Some(line) = line.filter(|line| *line > 0) {
        words.push(format!("+{line}"));
    }
    words.push(path.to_owned());
    words
}

/// The one line of shell a server runs, a constant: the link of the program `$0` under the home
/// folder, run with the other words as its arguments. Nothing of the path or the name is in it.
pub const SERVER_LINE: &str = r#"exec "$HOME/.local/share/bateri/apps/bin/$0" "$@""#;

/// What this computer runs: the program's link under `bin` (the folder of the programs this copy
/// follows has it), then the arguments. `None` for a name that is not a program's, or a link that
/// is not text.
pub fn argv_here(bin: &Path, name: &str, line: Option<u32>, path: &str) -> Option<Vec<String>> {
    if !apps::is_name(name) {
        return None;
    }
    let mut argv = vec![bin.join(name).to_str()?.to_owned()];
    argv.extend(arguments(line, path));
    Some(argv)
}

/// What a server runs: the constant line, the name as its `$0`, then the arguments. `None` for a
/// name that is not a program's.
pub fn argv_server(name: &str, line: Option<u32>, path: &str) -> Option<Vec<String>> {
    if !apps::is_name(name) {
        return None;
    }
    let mut argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        SERVER_LINE.to_owned(),
        name.to_owned(),
    ];
    argv.extend(arguments(line, path));
    Some(argv)
}

/// The folder the program starts in: the folder that was clicked, or the one a file is in.
pub fn directory(thing: Thing, path: &str) -> String {
    match thing {
        Thing::Folder => path.to_owned(),
        Thing::File => match path.rsplit_once('/') {
            Some(("", _)) => "/".to_owned(),
            Some((folder, _)) => folder.to_owned(),
            None => ".".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opener(name: &str, opens: &[Opens]) -> Opener {
        Opener {
            name: name.to_owned(),
            opens: opens.to_vec(),
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// Every row of the table: what a click on a file or a folder does for a setting, the
    /// programs that open paths, and what the machine has.
    #[test]
    fn a_click_opens_only_where_every_part_of_the_rule_agrees() {
        let both = [Opens::File, Opens::Folder];
        let openers = [
            opener("textpen", &both),
            opener("viewer", &[Opens::File]),
            opener("tree", &[Opens::Folder]),
        ];
        let has = names(&["textpen", "viewer", "tree", "other"]);
        let on = Some(has.as_slice());
        let table: &[(Thing, &str, Option<&str>)] = &[
            (Thing::File, "textpen", Some("textpen")),
            (Thing::Folder, "textpen", Some("textpen")),
            // Nothing: no path is a link.
            (Thing::File, "", None),
            (Thing::Folder, "", None),
            // A name that is not among the programs that open paths.
            (Thing::File, "other", None),
            (Thing::File, "missing", None),
            // The program takes only what it says.
            (Thing::File, "viewer", Some("viewer")),
            (Thing::Folder, "viewer", None),
            (Thing::File, "tree", None),
            (Thing::Folder, "tree", Some("tree")),
            // What is not a program's name is never looked up.
            (Thing::File, "../textpen", None),
            (Thing::File, "text pen", None),
            (Thing::File, "Textpen", None),
            (Thing::File, "-textpen", None),
        ];
        for (thing, setting, want) in table {
            assert_eq!(
                opens(*thing, setting, &openers, on).as_deref(),
                *want,
                "{thing:?} with {setting:?}"
            );
        }
    }

    #[test]
    fn a_machine_without_the_program_or_that_has_not_said_has_no_link() {
        let openers = [opener("textpen", &[Opens::File, Opens::Folder])];
        let without = names(&["notes"]);
        assert_eq!(
            opens(Thing::File, "textpen", &openers, Some(&without)),
            None
        );
        assert_eq!(opens(Thing::File, "textpen", &openers, Some(&[])), None);
        assert_eq!(opens(Thing::File, "textpen", &openers, None), None);
        let with = names(&["notes", "textpen"]);
        assert_eq!(
            opens(Thing::File, "textpen", &openers, Some(&with)).as_deref(),
            Some("textpen")
        );
    }

    #[test]
    fn the_choice_is_known_before_the_path_is() {
        let openers = [opener("viewer", &[Opens::File])];
        let choice = choose("viewer", &openers).expect("a program that opens files");
        assert!(choice.takes(Thing::File));
        assert!(!choice.takes(Thing::Folder));
        assert_eq!(choose("", &openers), None);
        assert_eq!(choose("textpen", &openers), None);
        assert_eq!(choose("viewer", &[]), None);
    }

    /// `path:12` is `+12`; a column, a line that is not one, and no line add nothing; the path is
    /// one word whatever is in it.
    #[test]
    fn the_words_are_the_program_a_line_and_the_path() {
        let bin = Path::new("/Users/me/.local/share/bateri/apps/bin");
        assert_eq!(
            argv_here(bin, "textpen", Some(12), "/src/main.rs").expect("words"),
            [
                "/Users/me/.local/share/bateri/apps/bin/textpen",
                "+12",
                "/src/main.rs"
            ]
        );
        for line in [None, Some(0)] {
            assert_eq!(
                argv_here(bin, "textpen", line, "/src/main.rs").expect("words"),
                [
                    "/Users/me/.local/share/bateri/apps/bin/textpen",
                    "/src/main.rs"
                ]
            );
        }
        for path in [
            "/tmp/a b/c d.txt",
            "/tmp/-rf",
            "/tmp/it's \"quoted\"",
            "/tmp/$(touch x);`y`",
            "/tmp/new\nline",
            "/tmp/+12",
        ] {
            let argv = argv_here(bin, "textpen", Some(3), path).expect("words");
            assert_eq!(argv.len(), 3, "{path:?}");
            assert_eq!(argv[2], path);
            let argv = argv_server("textpen", Some(3), path).expect("words");
            assert_eq!(argv.len(), 6, "{path:?}");
            assert_eq!(argv[5], path);
        }
    }

    /// A name that is not a program's makes no words; the server's line is one constant.
    #[test]
    fn the_names_that_make_words_are_programs_names() {
        let bin = Path::new("/apps/bin");
        for name in ["", "../x", "a b", "A", "-x", "x/y", "x;y", "$(x)"] {
            assert_eq!(argv_here(bin, name, None, "/p"), None, "{name:?}");
            assert_eq!(argv_server(name, None, "/p"), None, "{name:?}");
        }
        let argv = argv_server("textpen", Some(12), "/srv/it's here").expect("words");
        assert_eq!(
            argv,
            [
                "sh",
                "-c",
                r#"exec "$HOME/.local/share/bateri/apps/bin/$0" "$@""#,
                "textpen",
                "+12",
                "/srv/it's here"
            ]
        );
        // The line the server runs has nothing of the click in it.
        assert!(!SERVER_LINE.contains("textpen") && !SERVER_LINE.contains("/srv"));
    }

    #[test]
    fn a_program_starts_in_the_folder_clicked_or_the_one_the_file_is_in() {
        assert_eq!(directory(Thing::Folder, "/srv/app"), "/srv/app");
        assert_eq!(directory(Thing::File, "/srv/app/main.rs"), "/srv/app");
        assert_eq!(directory(Thing::File, "/main.rs"), "/");
        assert_eq!(directory(Thing::Folder, "/"), "/");
    }
}
