//! The wire of `ssh-hold`: what it reads and writes, byte for byte.

use super::*;

const SIZE: Size = Size {
    cols: 80,
    rows: 24,
    xpixel: 0,
    ypixel: 0,
};

fn id(name: &str) -> SessionId {
    SessionId::new(name).expect("a session id")
}

fn encoded(message: &ToApp) -> Vec<u8> {
    let mut out = Vec::new();
    message.encode(&mut out);
    out
}

fn encoded_back(message: &ToHold) -> Vec<u8> {
    let mut out = Vec::new();
    message.encode(&mut out);
    out
}

fn open_new() -> ToApp {
    ToApp::Open(Open {
        terminal: id("b-1"),
        size: SIZE,
        target: Target::New {
            args: vec![b"ssh".to_vec(), b"rasp".to_vec()],
        },
    })
}

fn open_resume(offset: Option<u64>) -> ToApp {
    ToApp::Open(Open {
        terminal: id("b-1"),
        size: SIZE,
        target: Target::Resume {
            account: "rasp".into(),
            offset,
        },
    })
}

/// Version 1 of the wire, frozen: every message this build can send, as the
/// bytes a build of any later time must still read. **Never edited**: a change
/// to the wire is a new version with a fixture of its own beside this one
/// (there is no version before 1; the range of versions read is
/// `OLDEST..=VERSION`, and this fixture is what keeps `OLDEST` honest).
const V1_FIXTURE: &[(&str, &[u8])] = &[
    ("hello", &[0x01, 0, 0, 0, 2, 1, 1]),
    (
        "open a new terminal",
        &[
            0x02, 0, 0, 0, 30, // frame
            3, b'b', b'-', b'1', // the terminal
            0, 80, 0, 24, 0, 0, 0, 0, // 80x24
            0, // new
            0, 2, // two arguments
            0, 0, 0, 3, b's', b's', b'h', //
            0, 0, 0, 4, b'r', b'a', b's', b'p',
        ],
    ),
    (
        "open an existing terminal from an offset",
        &[
            0x02, 0, 0, 0, 27, //
            3, b'b', b'-', b'1', //
            0, 80, 0, 24, 0, 0, 0, 0, //
            1, // resume
            4, b'r', b'a', b's', b'p', //
            1, 0, 0, 0, 0, 0, 0, 0x10, 0, // offset 4096
        ],
    ),
    (
        "open an existing terminal never seen",
        &[
            0x02, 0, 0, 0, 19, //
            3, b'b', b'-', b'1', //
            0, 80, 0, 24, 0, 0, 0, 0, //
            1, //
            4, b'r', b'a', b's', b'p', //
            0,
        ],
    ),
    ("input", &[0x03, 0, 0, 0, 3, b'l', b's', b'\r']),
    (
        "resize",
        &[0x04, 0, 0, 0, 8, 0, 100, 0, 30, 0x03, 0x20, 0x02, 0x58],
    ),
    ("welcome", &[0x81, 0, 0, 0, 1, 1]),
    ("refused, fall back", &[0x82, 0, 0, 0, 3, 1, b'n', b'o']),
    ("bound", &[0x83, 0, 0, 0, 5, 4, b'r', b'a', b's', b'p']),
    (
        "output",
        &[
            0x84, 0, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0, 5, b'h', b'e', b'l', b'l', b'o',
        ],
    ),
    ("end with a note", &[0x85, 0, 0, 0, 4, 7, b'b', b'y', b'e']),
    ("end", &[0x85, 0, 0, 0, 1, 0]),
    ("bell", &[0x86, 0, 0, 0, 0]),
];

fn fixture(name: &str) -> &'static [u8] {
    V1_FIXTURE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, bytes)| *bytes)
        .expect("a fixture of that name")
}

/// The messages the fixture holds, in the order the fixture lists them.
fn fixture_messages() -> (Vec<ToApp>, Vec<ToHold>) {
    (
        vec![
            ToApp::Hello {
                newest: 1,
                oldest: 1,
            },
            open_new(),
            open_resume(Some(4096)),
            open_resume(None),
            ToApp::Input(b"ls\r".to_vec()),
            ToApp::Resize(Size {
                cols: 100,
                rows: 30,
                xpixel: 800,
                ypixel: 600,
            }),
        ],
        vec![
            ToHold::Welcome { version: 1 },
            ToHold::Refused {
                fall_back: true,
                text: "no".into(),
            },
            ToHold::Bound {
                account: "rasp".into(),
                terminal: None,
            },
            ToHold::Output {
                end: 5,
                data: b"hello".to_vec(),
            },
            ToHold::End {
                code: 7,
                note: Some("bye".into()),
            },
            ToHold::End {
                code: 0,
                note: None,
            },
            ToHold::Bell,
        ],
    )
}

#[test]
fn version_one_is_frozen_byte_for_byte() {
    let (to_app, to_hold) = fixture_messages();
    let names_app = [
        "hello",
        "open a new terminal",
        "open an existing terminal from an offset",
        "open an existing terminal never seen",
        "input",
        "resize",
    ];
    for (message, name) in to_app.iter().zip(names_app) {
        assert_eq!(encoded(message), fixture(name), "writes {name}");
        let bytes = fixture(name);
        let parsed = ToApp::parse(bytes[0], &bytes[5..]).expect("reads");
        assert_eq!(parsed.as_ref(), Some(message), "reads {name}");
    }
    let names_hold = [
        "welcome",
        "refused, fall back",
        "bound",
        "output",
        "end with a note",
        "end",
        "bell",
    ];
    for (message, name) in to_hold.iter().zip(names_hold) {
        assert_eq!(encoded_back(message), fixture(name), "writes {name}");
        let bytes = fixture(name);
        let parsed = ToHold::parse(bytes[0], &bytes[5..]).expect("reads");
        assert_eq!(parsed.as_ref(), Some(message), "reads {name}");
    }
}

#[test]
fn the_versions_read_are_the_shipped_one_and_the_one_before() {
    assert_eq!(
        (OLDEST, VERSION),
        (1, 3),
        "nothing of version 1 or 2 has been shipped in a release, so both are still read"
    );
    // A holder of the same age, the one before, and one that also speaks later versions.
    assert_eq!(agree(3, 1), Some(3));
    assert_eq!(agree(2, 1), Some(2), "a pane that outlived an update");
    assert_eq!(agree(1, 1), Some(1), "a pane that outlived an update");
    assert_eq!(agree(7, 1), Some(VERSION), "the newest both speak");
    // A holder that has dropped everything this build speaks.
    assert_eq!(agree(9, 4), None);
    // A holder older than anything read (there is none yet; the rule is the range).
    assert_eq!(agree(0, 0), None);
}

/// Version 2 of the wire, frozen: what it added to version 1's messages.
/// **Never edited**; the next change is a version 3 and a fixture of its own.
const V2_FIXTURE: &[(&str, &[u8])] = &[
    ("hello of version 2", &[0x01, 0, 0, 0, 2, 2, 1]),
    ("welcome of version 2", &[0x81, 0, 0, 0, 1, 2]),
    (
        "run",
        &[
            0x87, 0, 0, 0, 23, //
            0, 3, // three arguments
            0, 0, 0, 3, b's', b's', b'h', //
            0, 0, 0, 2, b'-', b'M', //
            0, 0, 0, 4, b'r', b'a', b's', b'p',
        ],
    ),
    ("ran", &[0x05, 0, 0, 0, 1, 255]),
    ("note", &[0x88, 0, 0, 0, 3, b'h', b'i', b'!']),
];

#[test]
fn version_two_is_frozen_byte_for_byte() {
    let to_app = [
        (
            ToApp::Hello {
                newest: 2,
                oldest: 1,
            },
            "hello of version 2",
        ),
        (ToApp::Ran { status: 255 }, "ran"),
    ];
    for (message, name) in &to_app {
        let bytes = V2_FIXTURE
            .iter()
            .find(|(n, _)| n == name)
            .expect("a fixture")
            .1;
        assert_eq!(&encoded(message), bytes, "writes {name}");
        let parsed = ToApp::parse(bytes[0], &bytes[5..]).expect("reads");
        assert_eq!(parsed.as_ref(), Some(message), "reads {name}");
    }
    let to_hold = [
        (ToHold::Welcome { version: 2 }, "welcome of version 2"),
        (
            ToHold::Run {
                argv: vec![b"ssh".to_vec(), b"-M".to_vec(), b"rasp".to_vec()],
            },
            "run",
        ),
        (ToHold::Note { text: "hi!".into() }, "note"),
    ];
    for (message, name) in &to_hold {
        let bytes = V2_FIXTURE
            .iter()
            .find(|(n, _)| n == name)
            .expect("a fixture")
            .1;
        assert_eq!(&encoded_back(message), bytes, "writes {name}");
        let parsed = ToHold::parse(bytes[0], &bytes[5..]).expect("reads");
        assert_eq!(parsed.as_ref(), Some(message), "reads {name}");
    }
}

/// Version 3 of the wire, frozen: what it added to the earlier versions' messages. **Never
/// edited**; the next change is a version 4 and a fixture of its own.
const V3_FIXTURE: &[(&str, &[u8])] = &[
    ("hello of version 3", &[0x01, 0, 0, 0, 2, 3, 1]),
    ("welcome of version 3", &[0x81, 0, 0, 0, 1, 3]),
    // The account, then the name the terminal was started under.
    (
        "bound with a name",
        &[
            0x83, 0, 0, 0, 8, // frame
            4, b'r', b'a', b's', b'p', // the account
            2, b't', b'7', // the terminal
        ],
    ),
];

#[test]
fn version_three_is_frozen_byte_for_byte() {
    let hello = ToApp::Hello {
        newest: 3,
        oldest: 1,
    };
    assert_eq!(encoded(&hello), V3_FIXTURE[0].1, "writes hello");
    let bytes = V3_FIXTURE[0].1;
    assert_eq!(
        ToApp::parse(bytes[0], &bytes[5..]).expect("reads"),
        Some(hello)
    );
    let to_hold = [
        (ToHold::Welcome { version: 3 }, "welcome of version 3"),
        (
            ToHold::Bound {
                account: "rasp".into(),
                terminal: Some(id("t7")),
            },
            "bound with a name",
        ),
    ];
    for (message, name) in &to_hold {
        let bytes = V3_FIXTURE
            .iter()
            .find(|(n, _)| n == name)
            .expect("a fixture")
            .1;
        assert_eq!(&encoded_back(message), bytes, "writes {name}");
        let parsed = ToHold::parse(bytes[0], &bytes[5..]).expect("reads");
        assert_eq!(parsed.as_ref(), Some(message), "reads {name}");
    }
}

#[test]
fn a_bound_without_a_name_is_the_bytes_of_the_first_version_and_a_bad_name_is_a_broken_peer() {
    // What an older holder is sent, and reads, is exactly version 1's.
    let plain = ToHold::Bound {
        account: "rasp".into(),
        terminal: None,
    };
    assert_eq!(encoded_back(&plain), fixture("bound"));
    // A name that is no session id (a path), a name cut short, bytes after the name.
    let account = [4u8, b'r', b'a', b's', b'p'];
    for tail in [
        &[3u8, b'a', b'/', b'b'][..],
        &[0u8][..],
        &[5u8, b't', b'7'][..],
        &[2u8, b't', b'7', 0][..],
    ] {
        let mut payload = account.to_vec();
        payload.extend_from_slice(tail);
        assert_eq!(
            ToHold::parse(0x83, &payload),
            Err(Error::Malformed),
            "{tail:?}"
        );
    }
}

#[test]
fn a_run_with_too_many_arguments_or_a_cut_one_is_a_broken_peer() {
    // Two arguments announced, one given.
    let cut = [0u8, 2, 0, 0, 0, 1, b'x'];
    assert_eq!(ToHold::parse(0x87, &cut), Err(Error::Malformed));
    // Bytes after the last argument.
    let long = [0u8, 1, 0, 0, 0, 1, b'x', b'y'];
    assert_eq!(ToHold::parse(0x87, &long), Err(Error::Malformed));
    assert_eq!(ToApp::parse(0x05, &[]), Err(Error::Malformed));
}

#[test]
fn frames_come_out_of_any_pieces() {
    let (to_app, _) = fixture_messages();
    let mut all = Vec::new();
    for message in &to_app {
        message.encode(&mut all);
    }
    let mut reader = AppReader::new();
    let mut got = Vec::new();
    for byte in &all {
        reader.push(&[*byte]);
        while let Some(message) = reader.read().expect("whole") {
            got.push(message);
        }
    }
    assert_eq!(got, to_app);
    assert_eq!(reader.read(), Ok(None));
}

#[test]
fn a_frame_of_a_kind_nobody_here_knows_is_skipped() {
    let mut bytes = vec![0x7e, 0, 0, 0, 2, 9, 9];
    ToApp::Input(b"x".to_vec()).encode(&mut bytes);
    let mut reader = AppReader::new();
    reader.push(&bytes);
    assert_eq!(reader.read(), Ok(Some(ToApp::Input(b"x".to_vec()))));

    let mut bytes = vec![0xee, 0, 0, 0, 0];
    ToHold::Bell.encode(&mut bytes);
    let mut reader = HoldReader::new();
    reader.push(&bytes);
    assert_eq!(reader.read(), Ok(Some(ToHold::Bell)));
}

#[test]
fn a_broken_peer_is_an_error_and_not_a_wait() {
    // Announces more than the limit: refused before the payload is waited for.
    let mut reader = AppReader::new();
    let too_long = u32::try_from(MAX_PAYLOAD + 1).expect("fits").to_be_bytes();
    reader.push(&[0x03, too_long[0], too_long[1], too_long[2], too_long[3]]);
    assert_eq!(reader.read(), Err(Error::Oversize));

    // A payload that does not fit its kind.
    for broken in [
        &[0x01, 0, 0, 0, 1, 1][..],                // hello with one version
        &[0x04, 0, 0, 0, 3, 0, 1, 2][..],          // resize cut short
        &[0x02, 0, 0, 0, 2, 1, b'/'][..],          // a terminal name cut short
        &[0x02, 0, 0, 0, 5, 1, b'/', 0, 0, 0][..], // a name that is not a name
    ] {
        let mut reader = AppReader::new();
        reader.push(broken);
        assert_eq!(reader.read(), Err(Error::Malformed), "{broken:?}");
    }
    let mut reader = HoldReader::new();
    reader.push(&[0x83, 0, 0, 0, 2, 9, b'x']);
    assert_eq!(reader.read(), Err(Error::Malformed));
}

#[test]
fn a_long_stream_goes_as_several_frames_each_saying_where_it_stands() {
    let data = vec![b'x'; CHUNK * 2 + 10];
    let mut out = Vec::new();
    ToHold::Output {
        end: 1000 + data.len() as u64,
        data: data.clone(),
    }
    .encode(&mut out);
    let mut reader = HoldReader::new();
    reader.push(&out);
    let mut stands = 1000;
    let mut all = Vec::new();
    while let Some(message) = reader.read().expect("whole") {
        let ToHold::Output { end, data } = message else {
            panic!("only output");
        };
        stands += data.len() as u64;
        assert_eq!(end, stands, "a frame ends where its bytes end");
        all.extend_from_slice(&data);
    }
    assert_eq!(all, data);
    assert_eq!(stands, 1000 + data.len() as u64);

    let mut out = Vec::new();
    ToApp::Input(vec![b'y'; CHUNK + 1]).encode(&mut out);
    let mut reader = AppReader::new();
    reader.push(&out);
    let mut frames = 0;
    while reader.read().expect("whole").is_some() {
        frames += 1;
    }
    assert_eq!(frames, 2);
}

#[test]
fn a_frame_that_is_not_cut_says_the_position_it_was_given() {
    // A piece of a picture: more bytes than the position it stands at.
    let mut out = Vec::new();
    ToHold::Output {
        end: 10,
        data: vec![b'p'; 300],
    }
    .encode(&mut out);
    let mut reader = HoldReader::new();
    reader.push(&out);
    let Some(ToHold::Output { end, data }) = reader.read().expect("whole") else {
        panic!("output");
    };
    assert_eq!((end, data.len()), (10, 300));
}

#[test]
fn how_a_program_ended_is_the_status_a_shell_reports() {
    assert_eq!(exit_status(Exit::Code(0)), 0);
    assert_eq!(exit_status(Exit::Code(7)), 7);
    assert_eq!(exit_status(Exit::Code(-1)), 1);
    assert_eq!(exit_status(Exit::Code(300)), 1);
    assert_eq!(exit_status(Exit::Signal(9)), 137);
    assert_eq!(exit_status(Exit::Signal(1000)), 255);
}

#[test]
fn a_note_is_cut_at_a_character() {
    let long = "é".repeat(2000);
    let mut out = Vec::new();
    ToHold::End {
        code: 1,
        note: Some(long),
    }
    .encode(&mut out);
    let mut reader = HoldReader::new();
    reader.push(&out);
    let Ok(Some(ToHold::End {
        note: Some(note), ..
    })) = reader.read()
    else {
        panic!("an end with a note");
    };
    assert!(note.len() <= 1024 && note.chars().all(|c| c == 'é'));
}
