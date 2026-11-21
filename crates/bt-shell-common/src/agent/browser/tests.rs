use super::*;
use crate::kablo::local::LocalChannel;
use crate::settings::TempRoot;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;

const LEVELS: [CommandLevel; 4] = [
    CommandLevel::Off,
    CommandLevel::Own,
    CommandLevel::Screen,
    CommandLevel::Full,
];

fn every_action() -> Vec<Action> {
    let part = || "p1".to_owned();
    vec![
        Action::Ls,
        Action::Text { part: part() },
        Action::Snapshot { part: part() },
        Action::Shot { part: part() },
        Action::Click {
            target: "[3]".to_owned(),
        },
        Action::Type {
            text: "hi".to_owned(),
            into: None,
        },
        Action::Open {
            url: "https://localhost:3000/".to_owned(),
        },
        Action::Back,
        Action::Reload,
    ]
}

#[test]
fn the_four_that_read_see_and_the_five_that_do_something_change() {
    let seen: Vec<_> = every_action()
        .iter()
        .map(|action| (action.word(), act_of(action)))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("ls", Act::See),
            ("text", Act::See),
            ("snapshot", Act::See),
            ("shot", Act::See),
            ("click", Act::Change),
            ("type", Act::Change),
            ("open", Act::Change),
            ("back", Act::Change),
            ("reload", Act::Change),
        ]
    );
    // A verb the command lists has a row here: none is judged by default.
    assert_eq!(seen.len(), Action::WORDS.len());
}

fn verdict(level: CommandLevel, action: &Action, own: bool) -> Result<(), Kind> {
    judge(level, action, own, Some("prod"), "p9@prod").map_err(|failure| failure.kind)
}

/// One row of the table: a level, and what it says of a browser of its own machine's pane and of
/// another machine's, for reading and for changing.
#[test]
fn the_table_holds_row_by_row() {
    let see = Action::Text {
        part: "p1".to_owned(),
    };
    let change = Action::Click {
        target: "[3]".to_owned(),
    };
    // (level, see own, see other, change own, change other)
    let table = [
        (
            CommandLevel::Off,
            Err(Kind::Refused),
            Err(Kind::Refused),
            Err(Kind::Refused),
            Err(Kind::Refused),
        ),
        (
            CommandLevel::Own,
            Ok(()),
            Err(Kind::Unknown),
            Ok(()),
            Err(Kind::Unknown),
        ),
        // Another machine's browser is reached, and `screen` does not reach.
        (
            CommandLevel::Screen,
            Ok(()),
            Err(Kind::Refused),
            Ok(()),
            Err(Kind::Refused),
        ),
        (CommandLevel::Full, Ok(()), Ok(()), Ok(()), Ok(())),
    ];
    for (level, see_own, see_other, change_own, change_other) in table {
        assert_eq!(verdict(level, &see, true), see_own, "{level:?} sees own");
        assert_eq!(
            verdict(level, &see, false),
            see_other,
            "{level:?} sees other"
        );
        assert_eq!(
            verdict(level, &change, true),
            change_own,
            "{level:?} changes own"
        );
        assert_eq!(
            verdict(level, &change, false),
            change_other,
            "{level:?} changes other"
        );
    }
}

#[test]
fn every_verb_is_judged_by_the_act_it_is_and_none_gets_past_off() {
    for action in every_action() {
        for level in LEVELS {
            for own in [true, false] {
                let said = verdict(level, &action, own);
                let expected = match (level, own) {
                    (CommandLevel::Off, _) => Err(Kind::Refused),
                    (CommandLevel::Own, false) => Err(Kind::Unknown),
                    (CommandLevel::Screen, false) => Err(Kind::Refused),
                    _ => Ok(()),
                };
                assert_eq!(said, expected, "{} at {level:?}, own: {own}", action.word());
            }
        }
    }
}

#[test]
fn a_browser_of_another_machine_is_not_told_apart_from_one_that_is_not_there() {
    let failure = judge(
        CommandLevel::Own,
        &Action::Ls,
        false,
        Some("prod"),
        "p9@staging",
    )
    .unwrap_err();
    assert_eq!(failure.kind, Kind::Unknown);
    assert_eq!(failure.text, "there is no pane p9@staging");
}

#[test]
fn a_screen_level_is_told_that_only_full_reaches_another_machines_browser() {
    let failure = judge(
        CommandLevel::Screen,
        &Action::Text {
            part: "p1".to_owned(),
        },
        false,
        Some("prod"),
        "p9@staging",
    )
    .unwrap_err();
    assert_eq!(failure.kind, Kind::Refused);
    assert!(failure.text.contains("\"full\""), "{}", failure.text);
    assert!(
        failure.text.contains("level \"screen\""),
        "{}",
        failure.text
    );
    assert!(failure.text.contains("prod"), "{}", failure.text);
}

#[test]
fn a_refusal_names_the_level_and_the_setting_that_changes_it() {
    let failure = judge(CommandLevel::Off, &Action::Reload, true, Some("prod"), "p9").unwrap_err();
    assert_eq!(failure.kind, Kind::Refused);
    assert!(failure.text.contains("level \"off\""), "{}", failure.text);
    assert!(failure.text.contains("prod"), "{}", failure.text);
}

#[test]
fn an_element_is_a_node_a_snapshot_printed_or_a_point() {
    assert_eq!(aim("[12]").unwrap(), Aim::Node(12));
    assert_eq!(aim("12").unwrap(), Aim::Node(12));
    assert_eq!(aim(" [0] ").unwrap(), Aim::Node(0));
    assert_eq!(aim("412,88").unwrap(), Aim::Point(412.0, 88.0));
    assert_eq!(aim("10.5, 3").unwrap(), Aim::Point(10.5, 3.0));
    for bad in [
        "",
        "[]",
        "[x]",
        "[1",
        "e12",
        "-1",
        "1,",
        ",2",
        "1,2,3",
        "-3,4",
        "NaN,1",
        "inf,1",
        "1e9,1",
        "99999999999999999999",
    ] {
        let failure = aim(bad).expect_err(bad);
        assert_eq!(failure.kind, Kind::Usage, "{bad}");
        assert!(failure.text.contains("`X,Y`"), "{bad}: {}", failure.text);
    }
}

#[test]
fn a_browsers_id_is_written_in_hex_and_read_back() {
    for number in [0, 1, 0xdead_beef, u64::MAX] {
        assert_eq!(element_number(&element_name(number)).unwrap(), number);
    }
    assert_eq!(element_name(255), "ff");
    for bad in ["", "g1", "12345678901234567", "0x1f", "-1", "a b"] {
        assert_eq!(element_number(bad).unwrap_err().kind, Kind::Usage, "{bad}");
    }
}

fn listed(id: u64, keyboard: bool, url: &str, title: &str) -> Listed {
    Listed {
        id,
        status: "ready".to_owned(),
        url: url.to_owned(),
        title: title.to_owned(),
        keyboard,
    }
}

#[test]
fn an_ls_is_a_line_for_each_browser_with_the_keyboards_marked() {
    let text = listing(&[
        listed(0xa1, false, "https://example.com/", "Example"),
        listed(0xb2, true, "", ""),
    ]);
    assert_eq!(
        text,
        "\ta1\tready\thttps://example.com/\tExample\n*\tb2\tready\t\t\n"
    );
    assert_eq!(listing(&[]), "");
}

#[test]
fn a_title_with_a_line_break_in_it_stays_on_its_line_and_a_long_one_is_cut() {
    let text = listing(&[listed(1, false, "https://a/", "one\ntwo\tthree")]);
    assert_eq!(text, "\t1\tready\thttps://a/\tone two three\n");
    let long = "é".repeat(500);
    let text = listing(&[listed(1, false, "https://a/", &long)]);
    let title = text.trim_end().rsplit('\t').next().unwrap();
    assert_eq!(title.chars().count(), TITLE_SHOWN);
    assert!(title.ends_with('…'));
}

#[test]
fn an_ls_that_would_not_fit_a_reply_says_how_many_it_left_out() {
    let rows: Vec<_> = (0..5000)
        .map(|n| {
            listed(
                n,
                false,
                &format!("https://example.com/{n}"),
                &"t".repeat(100),
            )
        })
        .collect();
    let text = listing(&rows);
    assert!(text.len() <= TEXT_MAX, "{}", text.len());
    let last = text.lines().last().unwrap();
    assert!(last.starts_with("… and "), "{last}");
    let more: usize = last
        .trim_start_matches("… and ")
        .trim_end_matches(" more")
        .parse()
        .unwrap();
    assert_eq!(text.lines().count() - 1 + more, rows.len());
}

#[test]
fn a_text_that_fits_is_the_reply_and_one_that_does_not_is_a_file() {
    assert_eq!(
        said("hello\n".to_owned(), false),
        Said::Reply("hello\n".to_owned())
    );
    let exact = "x".repeat(TEXT_MAX);
    assert_eq!(said(exact.clone(), false), Said::Reply(exact));
    let over = "x".repeat(TEXT_MAX + 1);
    assert_eq!(said(over.clone(), false), Said::File(over.into_bytes()));
}

#[test]
fn a_text_the_helper_cut_is_a_file_that_says_so_at_its_end() {
    let Said::File(bytes) = said("short but cut".to_owned(), true) else {
        panic!("a cut text is a file");
    };
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("short but cut\n"), "{text}");
    assert!(text.contains("was cut here"), "{text}");
}

#[test]
fn base64_is_read_as_the_standard_alphabet_with_or_without_padding() {
    assert_eq!(unbase64("").unwrap(), b"");
    assert_eq!(unbase64("Zg==").unwrap(), b"f");
    assert_eq!(unbase64("Zm8=").unwrap(), b"fo");
    assert_eq!(unbase64("Zm9v").unwrap(), b"foo");
    assert_eq!(unbase64("Zm9vYg==").unwrap(), b"foob");
    assert_eq!(unbase64("Zm9vYg").unwrap(), b"foob");
    assert_eq!(unbase64("Zm9v\nYmFy\r\n").unwrap(), b"foobar");
    assert_eq!(unbase64("+/8=").unwrap(), [0xfb, 0xff]);
    // The PNG signature, as the helper writes a picture.
    assert_eq!(
        unbase64("iVBORw0KGgo=").unwrap(),
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
    );
    for bad in ["Z", "Zm9v=Y", "Zm9*", "Zh==", "Zm9vY", "-_8="] {
        assert_eq!(unbase64(bad), None, "{bad}");
    }
}

fn home(label: &str) -> (TempRoot, LocalChannel) {
    let root = TempRoot::new(label);
    let home = root.0.join("home");
    fs::create_dir_all(&home).expect("a home");
    (root, LocalChannel::new(home))
}

#[test]
fn a_file_is_written_whole_below_the_askers_home_and_is_the_accounts_alone() {
    let (_root, channel) = home("browser-file");
    let png = [0x89, b'P', b'N', b'G', 0, 1, 2, 3, 255];
    let filed = file_on(&channel, "1a2b-3c_0", FileKind::Png, &png).expect("written");
    assert_eq!(
        filed,
        Filed {
            kind: FileKind::Png,
            bytes: 9
        }
    );
    assert_eq!(filed.name("1a2b-3c_0"), "1a2b-3c_0.png");
    let folder = channel.home.join(DIR).join(BROWSER);
    let file = folder.join("1a2b-3c_0.png");
    assert_eq!(fs::read(&file).expect("the file"), png);
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
        0o700
    );
    // Nothing is left beside it.
    let names: Vec<_> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec!["1a2b-3c_0.png".to_owned()]);
}

#[test]
fn a_text_file_is_there_byte_for_byte_and_a_second_one_replaces_the_first() {
    let (_root, channel) = home("browser-text-file");
    let first = "line one\nline two — é\n";
    file_on(&channel, "t1", FileKind::Text, first.as_bytes()).expect("written");
    let file = channel.home.join(DIR).join(BROWSER).join("t1.txt");
    assert_eq!(fs::read_to_string(&file).unwrap(), first);
    let filed = file_on(&channel, "t1", FileKind::Text, b"x").expect("written");
    assert_eq!(filed.bytes, 1);
    assert_eq!(fs::read_to_string(&file).unwrap(), "x");
}

#[test]
fn a_part_that_is_not_the_commands_kind_of_word_is_never_given_to_a_shell() {
    let (_root, channel) = home("browser-part");
    for bad in [
        "",
        "a/b",
        "..",
        "a b",
        "a;b",
        "$(x)",
        "a\nb",
        "é",
        &"a".repeat(65),
    ] {
        let failure = file_on(&channel, bad, FileKind::Text, b"x").expect_err(bad);
        assert_eq!(failure.kind, Kind::Usage, "{bad:?}");
    }
    assert!(!channel.home.join(DIR).exists());
}

#[test]
fn a_folder_that_cannot_be_made_is_said() {
    let (_root, channel) = home("browser-unwritable");
    // A file where the folder's parent should be.
    fs::create_dir_all(channel.home.join(".local/share")).unwrap();
    fs::write(channel.home.join(DIR), b"in the way").unwrap();
    let failure = file_on(&channel, "p1", FileKind::Text, b"x").expect_err("unwritable");
    assert_eq!(failure.kind, Kind::Failed);
    assert!(failure.text.contains("folder"), "{}", failure.text);
}
