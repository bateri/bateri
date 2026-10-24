//! The strip over a program's view: what it says and where its parts stand.
//!
//! A program's view covers the whole pane, and a view that covers the pane could imitate the
//! terminal under it. So the application keeps a strip of its own at the top of the pane,
//! outside the view's area, that says what the view cannot say for itself, each thing once:
//! where the program runs (`⇄ prod`, the dock's mark for a server; nothing on this Mac), which
//! program it is (its icon, when it gave one, and its name), what the view is (its title, when it says more than the program's name), and
//! the way out — one button, **Quit ⌃C**, the verb and the key it is.
//!
//! Every name in it comes from the program or from the machine it runs on, so it is **cleaned
//! and bounded** here ([`strip`]) before a toolkit draws it: a name with a line break, a
//! direction override or a thousand characters must not look like the application's own words.
//! The title is the least trusted part (the program wrote it) and gives way first when the strip
//! is narrow; where the program runs stays last ([`arrange`]).
//!
//! Platform-free: the AppKit shell measures the text and draws the parts where [`arrange`] puts
//! them (`bt-shell-macos::pencere_view`), and another shell would do the same.

/// The strip's height, points: the height of the view's area is the pane's less this.
pub const HEIGHT: f64 = 32.0;

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

/// The side of the program's icon, points: a little over the height of the name's capitals, as
/// an application's icon stands by its name in a menu.
pub const ICON: f64 = 16.0;
/// The gap between the program's icon and its name, points: closer than [`GAP`], since the two
/// are one part and give way together.
pub const ICON_GAP: f64 = 6.0;

/// The gap left of the first part, points.
pub const LEFT: f64 = 14.0;
/// The gap between the parts, points: the parts are told apart by their colours and this, with
/// no separator between them.
pub const GAP: f64 = 8.0;
/// The gap right of the button, points.
pub const RIGHT: f64 = 8.0;
/// The narrowest a name is cut to before the next part gives way, points: a few characters and
/// the ellipsis. A design constant.
pub const MIN_PART: f64 = 40.0;

/// A part's place along the strip: where it starts and how wide it is, points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub x: f64,
    pub width: f64,
}

/// Where the parts stand, left to right; a part that is `None` is not drawn (it has no text, or
/// the strip is too narrow for it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub place: Option<Span>,
    /// The program's icon, just left of its name: it stays when the name is cut to nothing and
    /// goes when the program does.
    pub icon: Option<Span>,
    pub program: Option<Span>,
    pub title: Option<Span>,
    /// The button, at the right edge; it never gives way.
    pub button: Span,
}

/// The natural widths of the parts, as the toolkit measured them; a part with no text is 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Widths {
    pub place: f64,
    /// [`ICON`] when there is an icon to draw, else 0.
    pub icon: f64,
    pub program: f64,
    pub title: f64,
    pub button: f64,
}

/// Where the parts of a strip `width` points wide stand. The button is at the right edge; the
/// place, the program and the title stand in that order from the left, in what is left. When
/// they do not fit, the title is cut (to no less than [`MIN_PART`]) and then goes, then the
/// program is cut, and the place last — where the program runs is what a view must not lose to
/// a long title or a long name. The program's icon and its name are one part: the name is cut
/// before the icon, and the two go together.
pub fn arrange(width: f64, natural: Widths) -> Layout {
    let button_width = natural.button.max(0.0).min((width - LEFT - RIGHT).max(0.0));
    let button = Span {
        x: width - RIGHT - button_width,
        width: button_width,
    };
    // Up to the button, less the gap before it.
    let room = (button.x - GAP - LEFT).max(0.0);
    let (icon, name) = (natural.icon.max(0.0), natural.program.max(0.0));
    let joined = if icon > 0.0 && name > 0.0 {
        ICON_GAP
    } else {
        0.0
    };
    let [place, program, title] = fit(
        room,
        [
            natural.place.max(0.0),
            icon + joined + name,
            natural.title.max(0.0),
        ],
    );
    let mut x = LEFT;
    let mut put = |width: f64| {
        (width > 0.0).then(|| {
            let span = Span { x, width };
            x += width + GAP;
            span
        })
    };
    let place = put(place);
    let (icon, program) = match put(program) {
        Some(span) if icon > 0.0 => {
            let icon = icon.min(span.width);
            let rest = span.width - icon - ICON_GAP;
            let program = (name > 0.0 && rest > 0.0).then_some(Span {
                x: span.x + icon + ICON_GAP,
                width: rest,
            });
            (
                Some(Span {
                    x: span.x,
                    width: icon,
                }),
                program,
            )
        }
        program => (None, program),
    };
    Layout {
        place,
        icon,
        program,
        title: put(title),
        button,
    }
}

/// The widths of the place, the program and the title in `room` ([`arrange`]'s order of giving
/// way). A part of width 0 is not there and takes no gap.
fn fit(room: f64, [place, program, title]: [f64; 3]) -> [f64; 3] {
    let row = |parts: [f64; 3]| {
        let shown = parts.iter().filter(|width| **width > 0.0);
        shown.clone().sum::<f64>() + GAP * (shown.count().saturating_sub(1) as f64)
    };
    // What the parts before one leave for it, less the gap before it.
    let after = |before: [f64; 3]| {
        let used = row(before);
        room - used - if used > 0.0 { GAP } else { 0.0 }
    };
    if row([place, program, title]) <= room {
        return [place, program, title];
    }
    if title > 0.0 {
        let free = after([place, program, 0.0]);
        if free >= MIN_PART.min(title) || (place == 0.0 && program == 0.0) {
            return [place, program, free.clamp(0.0, title)];
        }
    }
    if program > 0.0 {
        let least = MIN_PART.min(program);
        let free = after([place, 0.0, 0.0]);
        if free >= least || place == 0.0 {
            return [place, free.clamp(0.0, program), 0.0];
        }
        let free = room - least - GAP;
        if free >= MIN_PART.min(place) {
            return [free, least, 0.0];
        }
    }
    [place.min(room), 0.0, 0.0]
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

    fn widths() -> Widths {
        Widths {
            place: 60.0,
            icon: 0.0,
            program: 100.0,
            title: 120.0,
            button: 70.0,
        }
    }

    fn span(x: f64, width: f64) -> Option<Span> {
        Some(Span { x, width })
    }

    #[test]
    fn a_wide_strip_holds_every_part_at_its_natural_width_in_order() {
        let layout = arrange(900.0, widths());
        assert_eq!(
            layout.button,
            Span {
                x: 822.0,
                width: 70.0
            }
        );
        assert_eq!(layout.place, span(14.0, 60.0));
        assert_eq!(layout.program, span(82.0, 100.0));
        assert_eq!(layout.title, span(190.0, 120.0));
    }

    #[test]
    fn a_part_with_no_text_takes_no_room() {
        let layout = arrange(
            900.0,
            Widths {
                place: 0.0,
                ..widths()
            },
        );
        assert_eq!(layout.place, None);
        assert_eq!(layout.program, span(14.0, 100.0));
        assert_eq!(layout.title, span(122.0, 120.0));
    }

    #[test]
    fn the_title_is_cut_first_and_then_goes() {
        // Room is 390 − 8 − 70 − 8 − 14 = 290: place and program take 168, and a gap 8.
        let layout = arrange(390.0, widths());
        assert_eq!(layout.place, span(14.0, 60.0));
        assert_eq!(layout.program, span(82.0, 100.0));
        assert_eq!(layout.title, span(190.0, 114.0));
        // Room 200: 24 would be left for the title, less than the least it is cut to.
        let layout = arrange(300.0, widths());
        assert_eq!(layout.title, None);
        assert_eq!(layout.program, span(82.0, 100.0));
    }

    #[test]
    fn the_program_is_cut_before_the_place_and_the_place_goes_last() {
        // Room 150: the place keeps 60, the program has 82.
        let layout = arrange(250.0, widths());
        assert_eq!(layout.title, None);
        assert_eq!(layout.place, span(14.0, 60.0));
        assert_eq!(layout.program, span(82.0, 82.0));
        // Room 90: the program keeps the least, the place what is left of it.
        let layout = arrange(190.0, widths());
        assert_eq!(
            layout.program,
            Some(Span {
                x: 64.0,
                width: MIN_PART
            })
        );
        assert_eq!(layout.place, span(14.0, 42.0));
        // Room 80: not both — the place stays, the program goes.
        let layout = arrange(180.0, widths());
        assert_eq!(layout.program, None);
        assert_eq!(layout.place, span(14.0, 60.0));
    }

    #[test]
    fn a_lone_part_is_cut_rather_than_dropped_and_nothing_goes_negative() {
        let lone = Widths {
            place: 0.0,
            icon: 0.0,
            program: 0.0,
            title: 300.0,
            button: 70.0,
        };
        assert_eq!(arrange(200.0, lone).title, span(14.0, 100.0));
        let pictured = Widths {
            icon: ICON,
            ..widths()
        };
        for width in [0.0, 10.0, 50.0, 100.0, 130.0, 160.0, 190.0, 250.0] {
            for natural in [widths(), lone, pictured] {
                let layout = arrange(width, natural);
                for part in [layout.place, layout.icon, layout.program, layout.title]
                    .into_iter()
                    .flatten()
                    .chain([layout.button])
                {
                    assert!(part.width >= 0.0, "{width}: {part:?}");
                }
            }
        }
    }

    #[test]
    fn the_icon_stands_left_of_the_program_and_the_rest_moves_over() {
        let layout = arrange(
            900.0,
            Widths {
                icon: ICON,
                ..widths()
            },
        );
        assert_eq!(layout.place, span(14.0, 60.0));
        assert_eq!(layout.icon, span(82.0, ICON));
        assert_eq!(layout.program, span(82.0 + ICON + ICON_GAP, 100.0));
        assert_eq!(
            layout.title,
            span(82.0 + ICON + ICON_GAP + 100.0 + GAP, 120.0)
        );
        // No icon, no room taken for one.
        assert_eq!(arrange(900.0, widths()).icon, None);
    }

    #[test]
    fn the_icon_gives_way_with_the_program_and_after_its_name() {
        let pictured = Widths {
            icon: ICON,
            ..widths()
        };
        // Room 150: the place keeps 60, the icon and the name share 82, the name is cut.
        let layout = arrange(250.0, pictured);
        assert_eq!(layout.icon, span(82.0, ICON));
        assert_eq!(
            layout.program,
            span(82.0 + ICON + ICON_GAP, 82.0 - ICON - ICON_GAP)
        );
        // Room 80: the program goes, and its icon with it.
        let layout = arrange(180.0, pictured);
        assert_eq!((layout.icon, layout.program), (None, None));
        assert_eq!(layout.place, span(14.0, 60.0));
        // An icon with no name stands alone where the program would.
        let layout = arrange(
            900.0,
            Widths {
                program: 0.0,
                ..pictured
            },
        );
        assert_eq!(layout.icon, span(82.0, ICON));
        assert_eq!(layout.program, None);
        assert_eq!(layout.title, span(82.0 + ICON + GAP, 120.0));
    }
}
