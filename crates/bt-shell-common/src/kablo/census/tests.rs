use super::*;

fn id(name: &str) -> SessionId {
    SessionId::new(name).expect("an id")
}

fn ids(names: &[&str]) -> Vec<SessionId> {
    names.iter().map(|name| id(name)).collect()
}

/// One terminal as the real `kablo list --json` writes it: every key, in its order, with a program
/// whose name looks like the keys that matter and an exit and a list of clients that are objects
/// and arrays of their own.
fn line(name: &str, state: &str, clients: u32, relay: &str, warden: &str) -> String {
    format!(
        "{{\"id\":\"{name}\",\"state\":\"{state}\",\"pid\":4242,\
         \"program\":\"vim \\\"x\\\",\\\"clients\\\":9,\\\"version\\\":\\\"9.9.9\\\"\",\
         \"directory\":null,\"cols\":80,\"rows\":24,\"xpixel\":0,\"ypixel\":0,\
         \"clients\":{clients},\"offset\":10,\"retained\":0,\"exit\":{{\"code\":0}},\
         \"version\":\"{relay}\",\"warden_version\":\"{warden}\",\"secret\":false,\
         \"label\":\"a \\u00e9 b\",\"started\":1,\"front_started\":null,\"front_is_own\":null,\
         \"output_at\":null,\"clients_named\":[\"a\",\"\"]}}"
    )
}

fn answer(dirs: &[&str], lines: &[String]) -> String {
    let listing = if lines.is_empty() {
        "[]\n".to_owned()
    } else {
        format!("[\n{}\n]\n", lines.join(",\n"))
    };
    format!(
        "{}{MARK}\n{listing}",
        dirs.iter().map(|d| format!("{d}\n")).collect::<String>()
    )
}

fn census(lines: &[String]) -> Census {
    Census::parse(&answer(&[], lines)).expect("a whole answer")
}

#[test]
fn a_listing_as_kablo_writes_it_is_read_key_by_key() {
    let lines = [
        line("p1", "running", 2, "0.9.0", "0.9.1"),
        line("t2", "ended", 0, "0.9.1", "0.9.1"),
    ];
    let seen = Census::parse(&answer(
        &[
            ".local/share/kablo/0.9.0",
            " .local/share/bateri/kablo/0.9.1 ",
        ],
        &lines,
    ))
    .expect("whole");
    assert_eq!(
        seen.dirs,
        [
            ".local/share/kablo/0.9.0",
            ".local/share/bateri/kablo/0.9.1"
        ]
    );
    assert_eq!(
        seen.terminals,
        [
            Terminal {
                id: id("p1"),
                running: true,
                clients: 2,
                version: "0.9.0".into(),
                warden_version: "0.9.1".into(),
            },
            Terminal {
                id: id("t2"),
                running: false,
                clients: 0,
                version: "0.9.1".into(),
                warden_version: "0.9.1".into(),
            },
        ]
    );
    // What a program says of itself is not a key: its name has `"clients":9` in it.
    assert!(seen.has(&id("p1")) && !seen.has(&id("p3")));
    assert!(seen.runs("0.9.0") && seen.runs("0.9.1") && !seen.runs("9.9.9"));
}

#[test]
fn a_machine_without_terminals_is_an_answer() {
    let seen = Census::parse(&answer(&[".local/share/bateri/kablo/0.9.1"], &[])).expect("whole");
    assert_eq!(seen.dirs.len(), 1);
    assert!(seen.terminals.is_empty());
}

#[test]
fn an_answer_that_is_not_a_whole_one_is_none() {
    let good = line("p1", "running", 0, "0.9.0", "0.9.0");
    let whole = answer(
        &[".local/share/bateri/kablo/0.9.1"],
        std::slice::from_ref(&good),
    );
    assert!(Census::parse(&whole).is_some(), "the control");
    // Cut short, anywhere.
    for end in [
        whole.len() - 2,
        whole.len() - 3,
        whole.len() - 40,
        whole.len() / 2,
    ] {
        assert!(Census::parse(&whole[..end]).is_none(), "cut at {end}");
    }
    for text in [
        "",
        "kablo: cannot connect\n",
        "---\n{}",
        "---\n[\n{\"id\":\"t1\"",
        "---\n[{}]",
        "---\n[1]",
        "---\n[{\"id\":\"p1\",\"version\":\"0.9.0\"}]",
        "---\n[{\"id\":\"p1\",\"warden_version\":\"0.9.0\"}]",
        // An id kablo would not have made.
        "---\n[{\"id\":\"a b\",\"version\":\"0.9.0\",\"warden_version\":\"0.9.0\"}]",
        "---\n[{\"id\":\"p1\",\"version\":\"0.9.0\",\"warden_version\":\"0.9.0\"}] x",
    ] {
        assert!(Census::parse(text).is_none(), "{text:?}");
    }
    // Not even the marker between the halves.
    assert!(Census::parse(&format!("[\n{good}\n]\n")).is_none());
}

#[test]
fn a_release_moves_what_runs_an_earlier_release_and_is_the_ledgers_or_idle() {
    let seen = census(&[
        line("ours", "running", 1, "0.9.0", "0.9.0"),
        line("idle", "running", 0, "0.9.0", "0.9.0"),
        line("theirs", "running", 2, "0.9.0", "0.9.0"),
        line("half", "running", 0, "0.9.1", "0.9.0"),
        line("current", "running", 0, "0.9.1", "0.9.1"),
        line("newer", "running", 0, "0.9.2", "0.9.2"),
        line("ahead", "running", 0, "0.9.1", "0.9.2"),
        line(
            "dev",
            "running",
            0,
            "0.9.0-dev+0123456789ab",
            "0.9.0-dev+0123456789ab",
        ),
        line("mixed", "running", 0, "0.9.0", "0.9.0-dev+0123456789ab"),
        line("ended", "ended", 0, "0.9.0", "0.9.0"),
        line("broken", "broken", 0, "0.9.0", "0.9.0"),
        line("odd", "running", 0, "nine", "0.9.0"),
        line(
            "ours-dev",
            "running",
            1,
            "0.9.0-dev+0123456789ab",
            "0.9.0-dev+0123456789ab",
        ),
    ]);
    let kept = ids(&["ours", "ended", "ours-dev", "newer"]);
    assert_eq!(
        seen.renewable(&kept, "0.9.1"),
        ids(&["ours", "idle", "half"]),
        "ours (open, but ours), idle, and a warden that is behind; never what somebody else has \
         open, what runs the package's release or a newer one, a build for development, one that \
         does not run, or a version that is not one"
    );
    // The same machine, from a release that is not behind anything.
    assert!(seen.renewable(&kept, "0.9.0").is_empty());
}

#[test]
fn a_build_for_development_moves_only_the_ledgers_and_never_back() {
    let seen = census(&[
        line("ours", "running", 1, "0.9.0", "0.9.0"),
        line(
            "ours-dev",
            "running",
            0,
            "0.9.1-dev+aaaaaaaaaaaa",
            "0.9.1-dev+aaaaaaaaaaaa",
        ),
        line(
            "ours-now",
            "running",
            0,
            "0.9.1-dev+bbbbbbbbbbbb",
            "0.9.1-dev+bbbbbbbbbbbb",
        ),
        line("ours-newer", "running", 0, "0.9.2", "0.9.2"),
        line("idle", "running", 0, "0.9.0", "0.9.0"),
    ]);
    let kept = ids(&["ours", "ours-dev", "ours-now", "ours-newer"]);
    assert_eq!(
        seen.renewable(&kept, "0.9.1-dev+bbbbbbbbbbbb"),
        ids(&["ours", "ours-dev"]),
        "the ledger's, but not the one that runs this build already, a newer release, or one \
         that nobody keeps"
    );
    // A package whose version is not one moves nothing.
    assert!(seen.renewable(&kept, "next").is_empty());
}
