//! What the application says over a program's view.
//!
//! A program's view covers the whole pane, and a view that covers the pane could imitate the
//! terminal under it. So the header over the pane — the application's own, outside the view's
//! area — says what the view cannot say for itself, each thing once, and for as long as the
//! view is up: where the program runs (`⇄ prod`, the dock's mark for a server; nothing on this
//! Mac), which program it is (its icon, when it gave one, and its name), what the view is (its
//! title, when it says more than the program's name), and the way out — one button,
//! **Quit ⌃C**, the verb and the key it is.
//!
//! Every name in it comes from the program or from the machine it runs on, so it is **cleaned
//! and bounded** here ([`strip`]) before a toolkit draws it: a name with a line break, a
//! direction override or a thousand characters must not look like the application's own words.
//! The title is the least trusted part (the program wrote it): the header says the place, the
//! program and the title in that order, so a header too narrow for them all loses the title
//! first and where the program runs last.
//!
//! Platform-free: the AppKit shell draws the words (`bt-shell-macos::tab_head`), and another
//! shell would do the same.

/// The mark before the server a program runs on: the dock's mark for a remote session
/// (`bt_core::dock::REMOTE_MARK`), so the strip names a server the way the dock does.
pub const REMOTE_MARK: char = '⇄';

/// The button's verb. The program is named once, left of it, not again in it.
pub const QUIT: &str = "Quit";

/// The key the button is, shown in it the way a menu shows a shortcut. The person's one way
/// out of a view that has stopped answering is ⌃C (the keys: the protocol's section on the
/// keyboard), and the button writes the same byte.
pub const QUIT_KEY: &str = "⌃C";

/// What the strip says when nothing names the view: no title, no program, on this Mac. A strip
/// with no words would not say that what is under it is a program's.
const UNNAMED: &str = "A program's view";

/// The most characters of a title the strip keeps. A design constant, not measured: a
/// sentence-long title would push the rest off the strip before the width could cut it.
pub const TITLE_LIMIT: usize = 48;

/// The most characters of a place or a program's name the strip keeps (the same reason).
pub const NAME_LIMIT: usize = 32;

/// What the strip says, each part ready to be drawn; a part that is `None` is not drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Strip {
    /// Where the program runs, `⇄ prod`, in the colour the dock gives a server; `None` on this
    /// Mac.
    pub place: Option<String>,
    /// The program, in the strong colour: what the person trusts and what the button quits.
    /// `None` when nothing here knows it (a plain `ssh`, a server that has not said).
    pub program: Option<String>,
    /// The view's title, in the quiet colour. `None` when there is none or it only says the
    /// program's name again.
    pub title: Option<String>,
    /// What the button does, for its tooltip: `Sends ⌃C to docker-ui. The shell comes back.`
    pub quit_tip: String,
}

/// The strip's text for a view the program called `title` in its `open`, if it did. `place` is
/// the server the program runs on, `None` on this Mac; `program` is the program's own name,
/// `None` when nothing here knows it (a plain `ssh`, a server that has not said).
///
/// A title with nothing left after cleaning is as if there were none, and so is one that is the
/// program's name in other letters (`Containers` from `containers`).
pub fn strip(title: Option<&str>, place: Option<&str>, program: Option<&str>) -> Strip {
    let named = |text: &str, limit: usize| Some(clean(text, limit)).filter(|text| !text.is_empty());
    let place = place.and_then(|text| named(text, NAME_LIMIT));
    let program = program.and_then(|text| named(text, NAME_LIMIT));
    let title = title
        .and_then(|text| named(text, TITLE_LIMIT))
        .filter(|title| {
            program
                .as_ref()
                .is_none_or(|program| title.to_lowercase() != program.to_lowercase())
        });
    let title = match (&place, &program, title) {
        (None, None, None) => Some(UNNAMED.to_owned()),
        (_, _, title) => title,
    };
    let quit_tip = format!(
        "Sends {QUIT_KEY} to {}. The shell comes back.",
        program.as_deref().unwrap_or("the program")
    );
    Strip {
        place: place.map(|place| format!("{REMOTE_MARK} {place}")),
        program,
        title,
        quit_tip,
    }
}

/// The name the strip calls the program by: what the program calls itself (`said`: the
/// `program` of its `open`), as long as something is left of it once it is cleaned, else
/// what the application reads in the pane (`read`: the process in front, the server's word for
/// a terminal it keeps). The program's own words are the ones a person knows it by (“Docker UI”
/// where the process is `node`); the questions to the person name it the same way.
pub fn program_name<'a>(said: Option<&'a str>, read: Option<&'a str>) -> Option<&'a str> {
    said.filter(|text| !clean(text, NAME_LIMIT).is_empty())
        .or(read)
}

/// `text` as the strip may show it: control characters gone, line breaks and runs of spaces one
/// space, the characters that hide or reverse text gone, and at most `limit` characters with a
/// trailing "…" when it was cut.
pub(crate) fn clean(text: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars() {
        if c.is_whitespace() {
            gap = !out.is_empty();
            continue;
        }
        if c.is_control() || hides_or_reverses(c) {
            continue;
        }
        if gap {
            out.push(' ');
            gap = false;
        }
        out.push(c);
    }
    if out.chars().count() <= limit {
        return out;
    }
    // One character of the limit is the ellipsis; a space or a joiner before it would dangle.
    let end = out
        .char_indices()
        .nth(limit.saturating_sub(1))
        .map_or(out.len(), |(at, _)| at);
    let kept = out[..end].trim_end_matches(|c: char| c.is_whitespace() || c == '\u{200d}');
    format!("{kept}…")
}

/// The characters that take no room but change what reads: zero-width spaces and marks, the
/// embeddings and overrides of text direction, and the byte-order mark. The zero-width joiner
/// is not among them: an emoji sequence is made with it.
fn hides_or_reverses(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200c}'
            | '\u{200e}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_on_this_mac_names_the_program_once_and_its_title_beside_it() {
        let strip = strip(Some("Table"), None, Some("tablepen"));
        assert_eq!(strip.place, None);
        assert_eq!(strip.program.as_deref(), Some("tablepen"));
        assert_eq!(strip.title.as_deref(), Some("Table"));
        assert_eq!(
            strip.quit_tip,
            "Sends ⌃C to tablepen. The shell comes back."
        );
    }

    #[test]
    fn a_view_on_a_server_says_where_the_program_runs_with_the_docks_mark() {
        let strip = strip(Some("Containers"), Some("prod"), Some("Docker UI"));
        assert_eq!(strip.place.as_deref(), Some("⇄ prod"));
        assert_eq!(strip.program.as_deref(), Some("Docker UI"));
        assert_eq!(strip.title.as_deref(), Some("Containers"));
    }

    #[test]
    fn a_title_that_only_says_the_programs_name_again_is_left_out() {
        let strip = strip(Some("Containers"), None, Some("containers"));
        assert_eq!(strip.program.as_deref(), Some("containers"));
        assert_eq!(strip.title, None);
    }

    #[test]
    fn a_program_nobody_here_can_name_is_not_given_a_name() {
        // A plain `ssh`: the place is known, the program is not.
        let strip = strip(Some("list"), Some("prod"), None);
        assert_eq!(strip.place.as_deref(), Some("⇄ prod"));
        assert_eq!(strip.program, None);
        assert_eq!(strip.title.as_deref(), Some("list"));
        assert_eq!(
            strip.quit_tip,
            "Sends ⌃C to the program. The shell comes back."
        );
    }

    #[test]
    fn a_strip_is_never_blank() {
        // A title of nothing is none: the program alone.
        let strip = super::strip(Some("  \n "), None, Some("tablepen"));
        assert_eq!(strip.title, None);
        assert_eq!(strip.program.as_deref(), Some("tablepen"));
        // Nothing at all names it, on this Mac: the strip still says what is under it.
        for title in [None, Some(" \u{200b}")] {
            let strip = super::strip(title, None, None);
            assert_eq!(strip.title.as_deref(), Some("A program's view"));
        }
        // On a server the place says it.
        assert_eq!(super::strip(None, Some("prod"), None).title, None);
    }

    #[test]
    fn the_strip_names_the_program_as_it_names_itself_and_the_view_as_it_titles_it() {
        // `node` is the process; “Docker UI” is what the program says it is, and “Containers” is
        // what it calls the view.
        let program = program_name(Some("Docker UI"), Some("node"));
        let strip = strip(Some("Containers"), None, program);
        assert_eq!(strip.program.as_deref(), Some("Docker UI"));
        assert_eq!(strip.title.as_deref(), Some("Containers"));
        assert_eq!(
            strip.quit_tip,
            "Sends ⌃C to Docker UI. The shell comes back."
        );
        // A program that gave no name is the process, or the server's word for it; one that gave
        // nothing that shows is as if it gave none.
        assert_eq!(program_name(None, Some("node")), Some("node"));
        assert_eq!(
            program_name(Some(" \u{200b}\n"), Some("node")),
            Some("node")
        );
        assert_eq!(program_name(None, None), None);
        assert_eq!(program_name(Some("\u{202e}"), None), None);
    }

    #[test]
    fn a_long_name_is_cut_and_a_title_that_imitates_the_application_is_cleaned() {
        let long = "x".repeat(200);
        let strip = strip(Some(&format!("t{long}")), Some(&long), Some(&long));
        let title = strip.title.expect("a title");
        assert_eq!(title.chars().count(), TITLE_LIMIT);
        assert!(title.ends_with('…'));
        let name = format!("{}…", "x".repeat(NAME_LIMIT - 1));
        assert_eq!(strip.place, Some(format!("⇄ {name}")));
        assert_eq!(strip.program, Some(name));

        // Line breaks, tabs and runs of spaces are one space; controls and the characters that
        // reverse or hide text are gone, so nothing in a name can start a second line, flip the
        // order of what follows it or fill the strip with what does not show.
        let strip = super::strip(
            Some("\u{202e}Quit\n\tbateri\u{200b}   now\u{7}"),
            Some("pro\u{0}d"),
            Some("a\u{2066}b"),
        );
        assert_eq!(strip.title.as_deref(), Some("Quit bateri now"));
        assert_eq!(strip.place.as_deref(), Some("⇄ prod"));
        assert_eq!(strip.program.as_deref(), Some("ab"));
    }

    #[test]
    fn a_cut_never_leaves_a_space_or_half_an_emoji_before_the_ellipsis() {
        let text = format!("{} {}", "a".repeat(TITLE_LIMIT - 2), "b".repeat(10));
        let cut = clean(&text, TITLE_LIMIT);
        assert!(cut.ends_with("a…"), "{cut}");
        // A joiner at the cut would be a half of an emoji sequence.
        let joined = format!("{}\u{200d}👩xx", "a".repeat(TITLE_LIMIT - 2));
        assert_eq!(
            clean(&joined, TITLE_LIMIT),
            format!("{}…", "a".repeat(TITLE_LIMIT - 2))
        );
        // Inside the limit a sequence is whole.
        assert_eq!(clean("👨\u{200d}👩", TITLE_LIMIT), "👨\u{200d}👩");
    }
}
