//! The strip over a program's view: what it says and where its parts stand.
//!
//! A program's view covers the whole pane, and a view that covers the pane could imitate the
//! terminal under it. So the application keeps a strip of its own at the top of the pane,
//! outside the view's area, that says three things the view cannot say for itself: that what is
//! shown is a program's view, where that program runs (the server's name; nothing on this Mac),
//! and how the person leaves it (⌃C, or the button that writes the same byte).
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

/// What the strip tells about leaving a view. The person's one way out of a view that has
/// stopped answering is ⌃C (the keys: the protocol's section on the keyboard).
pub const HINT: &str = "⌃C quits it, the shell comes back";

/// The words that say the view is the pane's own and not a window beside it.
const IN_THIS_PANE: &str = "in this pane";

/// The most characters of a title the strip keeps. A design constant, not measured: a
/// sentence-long title would push the rest off the strip before the width could cut it.
pub const TITLE_LIMIT: usize = 48;

/// The most characters of a place or a program's name the strip keeps (the same reason).
pub const NAME_LIMIT: usize = 32;

/// What the strip says, each part ready to be drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Strip {
    /// The view's title, in the strong colour.
    pub title: String,
    /// Where the program runs, the program and that the view is in this pane, in the quiet one:
    /// `prod · docker-ui · in this pane`, `docker-ui · in this pane` on this Mac.
    pub context: String,
    /// How the person leaves ([`HINT`]).
    pub hint: &'static str,
    /// The button that writes ⌃C: `Quit docker-ui`, a verb and what it ends.
    pub quit: String,
}

/// The strip's text for a view of `tool` (the name of the tool whose result the view shows)
/// that the program called `title` in its list of tools, if it did. `place` is the server the
/// program runs on, `None` on this Mac; `program` is the program's own name, `None` when
/// nothing here knows it (a plain `ssh`, a server that has not said).
///
/// A title the program did not give is the tool's name; a title or a name with nothing left
/// after cleaning is as if there were none.
pub fn strip(title: Option<&str>, tool: &str, place: Option<&str>, program: Option<&str>) -> Strip {
    let title = title
        .map(|text| clean(text, TITLE_LIMIT))
        .filter(|text| !text.is_empty())
        .or_else(|| Some(clean(tool, TITLE_LIMIT)).filter(|text| !text.is_empty()))
        .unwrap_or_default();
    let place = place
        .map(|text| clean(text, NAME_LIMIT))
        .filter(|text| !text.is_empty());
    let program = program
        .map(|text| clean(text, NAME_LIMIT))
        .filter(|text| !text.is_empty());
    let parts: Vec<&str> = place
        .iter()
        .chain(program.iter())
        .map(String::as_str)
        .chain(std::iter::once(IN_THIS_PANE))
        .collect();
    Strip {
        title,
        context: parts.join(" · "),
        hint: HINT,
        quit: match &program {
            Some(name) => format!("Quit {name}"),
            None => "Quit program".to_owned(),
        },
    }
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

/// The gap left of the icon, points.
pub const LEFT: f64 = 14.0;
/// The icon's side, points.
pub const ICON: f64 = 14.0;
/// The gap between the parts, points.
pub const GAP: f64 = 8.0;
/// The gap right of the button, points.
pub const RIGHT: f64 = 8.0;
/// The narrowest the title is cut to before the place and the program give way, points: a few
/// characters and the ellipsis. A design constant.
pub const MIN_TITLE: f64 = 40.0;

/// A part's place along the strip: where it starts and how wide it is, points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub x: f64,
    pub width: f64,
}

/// Where the parts stand, left to right; the icon is at [`LEFT`] and [`ICON`] wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub title: Span,
    pub context: Span,
    /// `None` when the strip is too narrow for it: it is the first thing to go.
    pub hint: Option<Span>,
    /// The button, at the right edge; it never gives way.
    pub button: Span,
}

/// The natural widths of the parts, as the toolkit measured them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Widths {
    pub title: f64,
    pub context: f64,
    pub hint: f64,
    pub button: f64,
}

/// Where the parts of a strip `width` points wide stand. The button is at the right edge and
/// the hint left of it; what is left between the icon and the hint is the title's and the
/// context's. When they do not fit, the hint goes first, then the title is cut (to no less than
/// [`MIN_TITLE`]), and only then the context — where the program runs is what a view must not
/// lose to a long title.
pub fn arrange(width: f64, natural: Widths) -> Layout {
    let start = LEFT + ICON + GAP;
    let button_width = natural.button.max(0.0).min((width - LEFT - RIGHT).max(0.0));
    let button = Span {
        x: width - RIGHT - button_width,
        width: button_width,
    };
    // Up to the button, less the gap before it.
    let room = (button.x - GAP - start).max(0.0);
    let (title, context) = (natural.title.max(0.0), natural.context.max(0.0));

    let with_hint = title + GAP + context + GAP + natural.hint;
    let without_hint = title + GAP + context;
    let (title_width, context_width, hint) = if room >= with_hint {
        (title, context, Some(natural.hint))
    } else if room >= without_hint {
        (title, context, None)
    } else {
        let title_least = MIN_TITLE.min(title);
        let cut_title = (room - GAP - context).max(title_least);
        if cut_title + GAP + context <= room {
            (cut_title, context, None)
        } else {
            (title_least, (room - GAP - title_least).max(0.0), None)
        }
    };
    Layout {
        title: Span {
            x: start,
            width: title_width,
        },
        context: Span {
            x: start + title_width + GAP,
            width: context_width,
        },
        hint: hint.map(|hint_width| Span {
            x: button.x - GAP - hint_width,
            width: hint_width,
        }),
        button,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_on_a_server_says_where_the_program_runs() {
        let strip = strip(
            Some("Containers"),
            "list_containers",
            Some("prod"),
            Some("docker-ui"),
        );
        assert_eq!(strip.title, "Containers");
        assert_eq!(strip.context, "prod · docker-ui · in this pane");
        assert_eq!(strip.hint, "⌃C quits it, the shell comes back");
        assert_eq!(strip.quit, "Quit docker-ui");
    }

    #[test]
    fn a_view_on_this_mac_names_no_place() {
        let strip = strip(
            Some("Containers"),
            "list_containers",
            None,
            Some("docker-ui"),
        );
        assert_eq!(strip.context, "docker-ui · in this pane");
        assert_eq!(strip.quit, "Quit docker-ui");
    }

    #[test]
    fn a_tool_with_no_title_is_named_by_the_tool() {
        let strip = strip(None, "list_containers", None, Some("docker-ui"));
        assert_eq!(strip.title, "list_containers");
        // A title of nothing is none.
        assert_eq!(
            super::strip(Some("  \n "), "list", None, None).title,
            "list"
        );
    }

    #[test]
    fn a_program_nobody_here_can_name_is_still_the_one_the_button_quits() {
        // A plain `ssh`: the place is known, the program is not.
        let strip = strip(None, "list", Some("prod"), None);
        assert_eq!(strip.context, "prod · in this pane");
        assert_eq!(strip.quit, "Quit program");
        let strip = super::strip(None, "list", None, None);
        assert_eq!(strip.context, "in this pane");
        assert_eq!(strip.quit, "Quit program");
    }

    #[test]
    fn a_long_name_is_cut_and_a_title_that_imitates_the_application_is_cleaned() {
        let long = "x".repeat(200);
        let strip = strip(Some(&long), "t", Some(&long), Some(&long));
        assert_eq!(strip.title.chars().count(), TITLE_LIMIT);
        assert!(strip.title.ends_with('…'));
        let name = format!("{}…", "x".repeat(NAME_LIMIT - 1));
        assert_eq!(strip.context, format!("{name} · {name} · in this pane"));
        assert_eq!(strip.quit, format!("Quit {name}"));

        // Line breaks, tabs and runs of spaces are one space; controls and the characters that
        // reverse or hide text are gone, so nothing in a name can start a second line, flip the
        // order of what follows it or fill the strip with what does not show.
        let strip = super::strip(
            Some("\u{202e}Quit\n\tbateri\u{200b}   now\u{7}"),
            "t",
            Some("pro\u{0}d"),
            Some("a\u{2066}b"),
        );
        assert_eq!(strip.title, "Quit bateri now");
        assert_eq!(strip.context, "prod · ab · in this pane");
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
            title: 80.0,
            context: 200.0,
            hint: 220.0,
            button: 100.0,
        }
    }

    #[test]
    fn a_wide_strip_holds_everything_at_its_natural_width() {
        let layout = arrange(900.0, widths());
        assert_eq!(
            layout.button,
            Span {
                x: 792.0,
                width: 100.0
            }
        );
        let hint = layout.hint.expect("room for the hint");
        assert_eq!(
            hint,
            Span {
                x: 564.0,
                width: 220.0
            }
        );
        assert_eq!(
            layout.title,
            Span {
                x: 36.0,
                width: 80.0
            }
        );
        assert_eq!(
            layout.context,
            Span {
                x: 124.0,
                width: 200.0
            }
        );
        // Nothing overlaps.
        assert!(layout.context.x + layout.context.width <= hint.x);
    }

    #[test]
    fn the_hint_is_the_first_to_go() {
        // 36 + 80 + 8 + 200 = 324 left of the gap before the button: the hint no longer fits.
        let layout = arrange(480.0, widths());
        assert_eq!(layout.hint, None);
        assert_eq!(layout.title.width, 80.0);
        assert_eq!(layout.context.width, 200.0);
    }

    #[test]
    fn the_title_is_cut_before_the_place_and_the_program() {
        let layout = arrange(400.0, widths());
        assert_eq!(layout.hint, None);
        assert_eq!(layout.context.width, 200.0);
        assert!(layout.title.width < 80.0 && layout.title.width >= MIN_TITLE);
        // The parts fill what is left and no more.
        let room = layout.button.x - GAP - 36.0;
        assert_eq!(layout.title.width + GAP + layout.context.width, room);
    }

    #[test]
    fn the_context_is_cut_last_and_nothing_goes_negative() {
        let layout = arrange(300.0, widths());
        assert_eq!(layout.title.width, MIN_TITLE);
        assert!(layout.context.width < 200.0 && layout.context.width > 0.0);
        let room = layout.button.x - GAP - 36.0;
        assert_eq!(layout.title.width + GAP + layout.context.width, room);
        for width in [0.0, 10.0, 50.0, 130.0, 160.0] {
            let layout = arrange(width, widths());
            for span in [layout.title, layout.context, layout.button] {
                assert!(span.width >= 0.0, "{width}: {span:?}");
            }
            assert!(layout.hint.is_none());
        }
    }

    #[test]
    fn a_short_title_is_never_widened() {
        let layout = arrange(
            300.0,
            Widths {
                title: 20.0,
                ..widths()
            },
        );
        assert_eq!(layout.title.width, 20.0);
    }
}
