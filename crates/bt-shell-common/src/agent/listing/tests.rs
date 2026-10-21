use kablo_proto::SessionId;
use kablo_proto::v1::{Exit, Size, State, Status};

use super::*;

const NOW: u64 = 1_000_000;

fn status(id: &str, program: &str) -> Status {
    let mut status = Status::new(SessionId::new(id).expect("a valid id"));
    status.program = Some(program.as_bytes().to_vec());
    status.size = Size {
        cols: 120,
        rows: 30,
        xpixel: 0,
        ypixel: 0,
    };
    status.state = State::RUNNING;
    status
}

fn row(status: Status) -> Row {
    Row {
        status,
        opened_by: None,
        waiting: false,
    }
}

fn terminal(name: &str) -> Pane {
    Pane::Terminal(name.to_owned())
}

/// The tree of a tab whose panes are told by `names` (a pane not in it is another machine).
fn tree(shape: &Tree, names: &[(u64, &str)]) -> Node {
    Node::of(shape, &|id| {
        names
            .iter()
            .find(|(pane, _)| *pane == id)
            .map_or(Pane::Elsewhere, |(_, name)| terminal(name))
    })
}

fn pair(axis: Axis, ratio: f64, first: Tree, second: Tree) -> Tree {
    Tree::Split {
        axis,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

fn leaf(id: u64) -> Tree {
    Tree::Leaf(id)
}

/// A scene with every kind of thing in it.
fn scene() -> Scene {
    let first = Tab {
        keys: Some("⌘1".to_owned()),
        name: Some("api".to_owned()),
        selected: true,
        zoomed: None,
        tree: tree(
            &pair(
                Axis::Horizontal,
                0.5,
                leaf(1),
                pair(Axis::Vertical, 0.6, leaf(2), leaf(3)),
            ),
            &[(1, "t4"), (2, "t7"), (3, "t9")],
        ),
    };
    // Zoomed, and not the selected tab: nothing in it is on screen.
    let zoomed = Tab {
        keys: Some("⌘2".to_owned()),
        name: None,
        selected: false,
        zoomed: Some(terminal("t12")),
        tree: tree(
            &pair(Axis::Horizontal, 0.5, leaf(4), leaf(5)),
            &[(4, "t12"), (5, "t13")],
        ),
    };
    // No shortcut, and a side that is nothing of this server's folds into one node.
    let grouped = Tab {
        keys: None,
        name: Some("logs".to_owned()),
        selected: false,
        zoomed: None,
        tree: tree(
            &pair(
                Axis::Horizontal,
                0.3,
                leaf(6),
                pair(Axis::Vertical, 0.5, leaf(7), leaf(8)),
            ),
            &[(6, "t15")],
        ),
    };
    // Nothing of this server's at all: one line, no name.
    let foreign = Tab {
        keys: Some("⌘4".to_owned()),
        name: None,
        selected: false,
        zoomed: None,
        tree: tree(&pair(Axis::Horizontal, 0.5, leaf(9), leaf(10)), &[]),
    };
    let behind = Tab {
        keys: Some("⌘1".to_owned()),
        name: None,
        selected: true,
        zoomed: None,
        tree: tree(&leaf(11), &[(11, "t30")]),
    };

    let mut shell = status("t4", "zsh");
    shell.started = Some(NOW - 3700);
    shell.front_is_own = Some(true);
    shell.output_at = Some(NOW - 5);
    shell.cwd = Some(b"/home/ada/api".to_vec());

    let mut htop = status("t7", "htop");
    htop.started = Some(NOW - 900);
    htop.front_is_own = Some(false);
    htop.front_started = Some(NOW - 125);
    htop.output_at = Some(NOW);
    htop.cwd = Some(b"/home/ada".to_vec());

    let mut ssh = status("t9", "ssh");
    ssh.secret = true;
    ssh.started = Some(NOW - 12);
    ssh.front_is_own = Some(false);
    ssh.front_started = Some(NOW - 12);
    ssh.output_at = Some(NOW - 12);

    let mut ended = status("t12", "zsh");
    ended.state = State::ENDED;
    ended.exit = Some(Exit::Code(1));
    ended.started = Some(NOW - 400);
    ended.output_at = Some(NOW - 300);
    ended.label = Some("build".to_owned());

    let mut killed = status("t13", "make");
    killed.state = State::ENDED;
    killed.exit = Some(Exit::Signal(9));

    let mut broken = status("t14", "zsh");
    broken.state = State::BROKEN;

    // The relay says nothing of the times: it runs, and that is all.
    let old = status("t15", "node");

    // A program started in place of the shell: in front, and its own, and not idle.
    let mut dev = status("t30", "node");
    dev.started = Some(NOW - 60);
    dev.front_is_own = Some(true);
    dev.output_at = Some(NOW - 1);
    dev.cwd = Some(b"/srv/app".to_vec());

    // No pane has it: it is in the list of what runs in the background.
    let mut listed = status("t41", "zsh");
    listed.started = Some(NOW);
    listed.front_is_own = Some(true);
    listed.output_at = Some(NOW);

    // A terminal a person made with kablo itself.
    let mut named = status("b-0a1b", "zsh");
    named.started = Some(NOW - 90_000);
    named.front_is_own = Some(true);
    named.output_at = Some(NOW - 7200);

    let opened = |status: Status, by: &str| Row {
        opened_by: Some(by.to_owned()),
        ..row(status)
    };
    Scene {
        caller: Some("t4".to_owned()),
        typing: Some("t7".to_owned()),
        windows: Some(vec![
            Window {
                visible: true,
                tabs: vec![first, zoomed, grouped, foreign],
            },
            Window {
                visible: false,
                tabs: vec![behind],
            },
        ]),
        // Not in the order the table prints them.
        terminals: vec![
            row(named),
            opened(dev, "t4"),
            row(htop),
            opened(listed, "t4"),
            row(shell),
            opened(ended, "t4"),
            row(ssh),
            row(killed),
            row(broken),
            row(old),
        ],
    }
}

#[test]
fn the_scene_is_told_as_windows_tabs_and_trees_and_then_a_table() {
    let expected = "\
You are t4. The user is typing in t7.

Window 1
  Tab 1.1 ⌘1 \"api\" (selected)
    side by side 50/50
      t4 (on screen)
      stacked 60/40
        t7 (on screen)
        t9 (on screen)
  Tab 1.2 ⌘2 (zoomed on t12)
    side by side 50/50
      t12
      t13
  Tab 1.3 \"logs\"
    side by side 30/70
      t15
      another machine
  Tab 1.4 ⌘4 another machine
Window 2 (hidden)
  Tab 2.1 ⌘1 (selected)
    t30

ID      TAB   LABEL  PROGRAM  STATE            AGE      QUIET   SIZE    OPENED BY  DIRECTORY
t4      1.1   -      zsh      idle             1h 01m   5s      120x30  -          /home/ada/api
t7      1.1   -      htop     running 2m 05s   15m 00s  0s      120x30  -          /home/ada
t9      1.1   -      ssh      password prompt  12s      12s     120x30  -          -
t12     1.2   build  zsh      exited 1         6m 40s   5m 00s  120x30  t4         -
t13     1.2   -      make     signal 9         -        -       120x30  -          -
t14     none  -      zsh      broken           -        -       120x30  -          -
t15     1.3   -      node     running          -        -       120x30  -          -
t30     2.1   -      node     running 1m 00s   1m 00s   1s      120x30  t4         /srv/app
t41     none  -      zsh      idle             0s       0s      120x30  t4         -
b-0a1b  none  -      zsh      idle             25h 00m  2h 00m  120x30  -          -
";
    assert_eq!(scene().text(NOW), expected);
}

#[test]
fn an_idle_terminal_is_a_shell_waiting_and_a_program_in_its_place_is_running() {
    let at = |program: &str, own: Option<bool>| {
        let mut one = status("t1", program);
        one.started = Some(NOW - 30);
        one.front_started = Some(NOW - 20);
        one.front_is_own = own;
        Doing::of(&one, NOW).text()
    };
    assert_eq!(at("zsh", Some(true)), "idle");
    assert_eq!(
        at("-zsh", Some(true)),
        "idle",
        "a login shell is named with a dash"
    );
    assert_eq!(at("fish", Some(true)), "idle");
    assert_eq!(
        at("vim", Some(true)),
        "running 20s",
        "its own program, from when it began"
    );
    assert_eq!(
        at("vim", Some(false)),
        "running 20s",
        "from when the program in front began"
    );
    let mut unread = status("t1", "vim");
    unread.started = Some(NOW - 30);
    unread.front_is_own = Some(true);
    assert_eq!(
        Doing::of(&unread, NOW).text(),
        "running 30s",
        "its own program, from the terminal's start when the front one's is not known"
    );
    assert_eq!(at("zsh", None), "running");
    let mut nameless = status("t1", "zsh");
    nameless.program = None;
    nameless.front_is_own = Some(true);
    assert_eq!(
        Doing::of(&nameless, NOW).text(),
        "running",
        "a program that is not named is not known to be a shell"
    );
    // A password prompt is a prompt whatever else the times say, and only while it runs.
    let mut asking = status("t1", "zsh");
    asking.secret = true;
    asking.front_is_own = Some(true);
    assert_eq!(Doing::of(&asking, NOW).text(), "password prompt");
    asking.state = State::ENDED;
    assert_eq!(Doing::of(&asking, NOW).text(), "exited");
    asking.state = State(9);
    assert_eq!(Doing::of(&asking, NOW).text(), "unknown");
}

#[test]
fn the_times_are_the_cards_and_the_counters() {
    // The same reading of a status as the card's rows make of a terminal in the background.
    let mut one = status("t1", "htop");
    one.front_is_own = Some(false);
    one.front_started = Some(NOW - 125);
    assert_eq!(
        crate::server_card::running_text(Some(Duration::from_secs(125))),
        "running 2m 05s"
    );
    assert_eq!(Doing::of(&one, NOW).text(), "running 2m 05s");
    // Below a second there is no counter: `running`, and an age of `0s`.
    one.front_started = Some(NOW);
    assert_eq!(Doing::of(&one, NOW).text(), "running");
    assert_eq!(since(NOW, Some(NOW)), "0s");
    // A clock that is behind is not a time in the future, and one that is far off is bounded.
    assert_eq!(since(NOW, Some(NOW + 50)), "0s");
    assert_eq!(since(NOW, None), "-");
    assert!(since(u64::MAX, Some(0)).ends_with('m'));
}

#[test]
fn a_tab_without_a_pane_of_the_servers_is_one_line_and_a_side_of_other_machines_is_one_node() {
    let text = scene().text(NOW);
    assert_eq!(text.matches("another machine").count(), 2);
    assert!(text.contains("  Tab 1.4 ⌘4 another machine\n"));
    // A tab the user named keeps its name even when nothing in it is the server's; the builder
    // decides what a tab may be called, the listing prints it.
    let mut named = scene();
    if let Some(windows) = &mut named.windows {
        windows[0].tabs[3].name = Some("Deploy".to_owned());
    }
    assert!(
        named
            .text(NOW)
            .contains("  Tab 1.4 ⌘4 \"Deploy\" another machine\n")
    );
    // Whoever built the tree, the folding is the listing's.
    let loose = Node::Split {
        axis: Axis::Vertical,
        ratio: 0.5,
        first: Box::new(Node::Pane(Pane::Elsewhere)),
        second: Box::new(Node::Pane(Pane::Elsewhere)),
    };
    assert_eq!(loose.folded(), Node::Pane(Pane::Elsewhere));
}

#[test]
fn a_terminal_on_screen_is_in_a_visible_window_in_the_selected_tab_and_not_behind_a_zoom() {
    let shape = pair(Axis::Horizontal, 0.5, leaf(1), leaf(2));
    let names = [(1, "t1"), (2, "t2")];
    let one = |visible, selected, zoomed: Option<Pane>| {
        let scene = Scene {
            caller: None,
            typing: None,
            windows: Some(vec![Window {
                visible,
                tabs: vec![Tab {
                    keys: None,
                    name: None,
                    selected,
                    zoomed,
                    tree: tree(&shape, &names),
                }],
            }]),
            terminals: Vec::new(),
        };
        let text = scene.text(NOW);
        (
            text.contains("t1 (on screen)"),
            text.contains("t2 (on screen)"),
        )
    };
    assert_eq!(one(true, true, None), (true, true));
    assert_eq!(
        one(false, true, None),
        (false, false),
        "the window is not visible"
    );
    assert_eq!(
        one(true, false, None),
        (false, false),
        "the tab is not selected"
    );
    assert_eq!(
        one(true, true, Some(terminal("t2"))),
        (false, true),
        "t1 is behind the zoom"
    );
    assert_eq!(
        one(true, true, Some(Pane::Elsewhere)),
        (false, false),
        "the zoom is another machine's pane: nothing of this server's is shown"
    );
}

#[test]
fn a_text_from_the_server_cannot_move_the_readers_terminal() {
    let mut hostile = status("t1", "zsh\u{1b}]0;pwned\u{7}");
    hostile.label = Some("a\u{1b}[31mb".to_owned());
    hostile.cwd = Some(b"/tmp/\x1b[2Jx".to_vec());
    let scene = Scene {
        caller: None,
        typing: None,
        windows: None,
        terminals: vec![Row {
            opened_by: Some("t0\n\u{1b}".to_owned()),
            ..row(hostile)
        }],
    };
    let text = scene.text(NOW);
    assert!(
        text.chars().all(|c| c == '\n' || !c.is_control()),
        "{text:?}"
    );
    assert!(text.contains("zsh?]0;pwned?"), "{text}");
    assert!(text.contains("a?[31mb"), "{text}");
    // JSON escapes them instead.
    let json = scene.json();
    assert!(json.contains("\\u001b"), "{json}");
    assert!(
        json.chars().all(|c| c == '\n' || !c.is_control()),
        "{json:?}"
    );
}

#[test]
fn with_the_mac_away_the_table_stands_alone() {
    let mut waiting = row(status("t9", "npm"));
    waiting.waiting = true;
    waiting.opened_by = Some("t4".to_owned());
    let scene = Scene {
        caller: Some("t4".to_owned()),
        typing: None,
        windows: None,
        terminals: vec![waiting, row(status("t4", "zsh"))],
    };
    let expected = "\
You are t4.

The Mac is not connected, so what is on screen is not known.

ID  TAB      LABEL  PROGRAM  STATE    AGE  QUIET  SIZE    OPENED BY  DIRECTORY
t4  -        -      zsh      running  -    -      120x30  -          -
t9  waiting  -      npm      running  -    -      120x30  t4         -
";
    assert_eq!(scene.text(NOW), expected);
    let json = scene.json();
    let parsed = Json::parse(&json);
    assert_eq!(parsed.get("windows"), &Json::Null);
    let terminals = parsed.get("terminals").items();
    assert_eq!(terminals[0].get("placed").string(), "unknown");
    assert_eq!(terminals[1].get("placed").string(), "waiting");
}

#[test]
fn small_scenes_say_what_is_missing() {
    let empty = Scene {
        caller: None,
        typing: None,
        windows: Some(Vec::new()),
        terminals: Vec::new(),
    };
    assert_eq!(empty.text(NOW), "No windows are open.\n\nNo terminals.\n");
    let alone = Scene {
        caller: Some("t3".to_owned()),
        typing: Some("t3".to_owned()),
        ..empty.clone()
    };
    assert!(
        alone
            .text(NOW)
            .starts_with("You are t3, and the user is typing in it.\n\n")
    );
    let typing = Scene {
        typing: Some("t8".to_owned()),
        ..empty
    };
    assert!(
        typing
            .text(NOW)
            .starts_with("The user is typing in t8.\n\nNo windows")
    );
}

// ─── JSON ────────────────────────────────────────────────────────────────

/// Just enough JSON to hold the output to the grammar: the tests do not trust the writer to
/// check the writer.
#[derive(Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn parse(text: &str) -> Json {
        let mut at = Reader {
            chars: text.chars().collect(),
            at: 0,
        };
        let value = at.value();
        at.space();
        assert_eq!(at.at, at.chars.len(), "trailing text after the value");
        value
    }

    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Object(pairs) => pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
                .unwrap_or_else(|| panic!("no key {key:?} in {self:?}")),
            other => panic!("{other:?} has no keys"),
        }
    }

    fn items(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            other => panic!("{other:?} is no array"),
        }
    }

    fn string(&self) -> &str {
        match self {
            Json::String(text) => text,
            other => panic!("{other:?} is no string"),
        }
    }

    fn keys(&self) -> Vec<&str> {
        match self {
            Json::Object(pairs) => pairs.iter().map(|(name, _)| name.as_str()).collect(),
            other => panic!("{other:?} has no keys"),
        }
    }
}

struct Reader {
    chars: Vec<char>,
    at: usize,
}

impl Reader {
    fn space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn take(&mut self, c: char) {
        self.space();
        assert_eq!(
            self.chars.get(self.at),
            Some(&c),
            "expected {c:?} at {}",
            self.at
        );
        self.at += 1;
    }

    fn word(&mut self, word: &str) {
        for c in word.chars() {
            assert_eq!(
                self.chars.get(self.at),
                Some(&c),
                "in {word:?} at {}",
                self.at
            );
            self.at += 1;
        }
    }

    fn value(&mut self) -> Json {
        self.space();
        match self.chars.get(self.at).copied() {
            Some('n') => {
                self.word("null");
                Json::Null
            }
            Some('t') => {
                self.word("true");
                Json::Bool(true)
            }
            Some('f') => {
                self.word("false");
                Json::Bool(false)
            }
            Some('"') => Json::String(self.string()),
            Some('[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.chars.get(self.at) == Some(&']') {
                    self.at += 1;
                    return Json::Array(items);
                }
                loop {
                    items.push(self.value());
                    self.space();
                    match self.chars.get(self.at) {
                        Some(',') => self.at += 1,
                        Some(']') => {
                            self.at += 1;
                            return Json::Array(items);
                        }
                        other => panic!("in an array: {other:?} at {}", self.at),
                    }
                }
            }
            Some('{') => {
                self.at += 1;
                let mut pairs = Vec::new();
                self.space();
                if self.chars.get(self.at) == Some(&'}') {
                    self.at += 1;
                    return Json::Object(pairs);
                }
                loop {
                    self.space();
                    let key = self.string();
                    self.take(':');
                    pairs.push((key, self.value()));
                    self.space();
                    match self.chars.get(self.at) {
                        Some(',') => self.at += 1,
                        Some('}') => {
                            self.at += 1;
                            return Json::Object(pairs);
                        }
                        other => panic!("in an object: {other:?} at {}", self.at),
                    }
                }
            }
            Some(c) if c == '-' || c.is_ascii_digit() => {
                let start = self.at;
                while self
                    .chars
                    .get(self.at)
                    .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | 'e' | 'E' | '+'))
                {
                    self.at += 1;
                }
                let text: String = self.chars[start..self.at].iter().collect();
                Json::Number(text.parse().unwrap_or_else(|_| panic!("number {text:?}")))
            }
            other => panic!("a value was expected: {other:?} at {}", self.at),
        }
    }

    fn string(&mut self) -> String {
        self.take('"');
        let mut out = String::new();
        loop {
            let c = *self.chars.get(self.at).expect("a string that ends");
            self.at += 1;
            match c {
                '"' => return out,
                '\\' => {
                    let escape = *self.chars.get(self.at).expect("an escape");
                    self.at += 1;
                    out.push(match escape {
                        '"' => '"',
                        '\\' => '\\',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'u' => {
                            let digits: String = self.chars[self.at..self.at + 4].iter().collect();
                            self.at += 4;
                            char::from_u32(u32::from_str_radix(&digits, 16).expect("hex"))
                                .expect("a scalar value")
                        }
                        other => panic!("escape {other:?}"),
                    });
                }
                c => {
                    assert!(
                        !c.is_control(),
                        "a raw control character in a string: {c:?}"
                    );
                    out.push(c);
                }
            }
        }
    }
}

#[test]
fn the_json_is_json_and_says_what_the_text_says() {
    let json = scene().json();
    let parsed = Json::parse(&json);
    assert_eq!(parsed.get("caller").string(), "t4");
    assert_eq!(parsed.get("typing").string(), "t7");

    let windows = parsed.get("windows").items();
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].get("number"), &Json::Number(1.0));
    assert_eq!(windows[0].get("visible"), &Json::Bool(true));
    assert_eq!(windows[1].get("visible"), &Json::Bool(false));
    let tabs = windows[0].get("tabs").items();
    assert_eq!(tabs.len(), 4);

    // The tree is the tree: an axis, the first share, two sides.
    let root = tabs[0].get("tree");
    assert_eq!(root.keys(), ["axis", "ratio", "first", "second"]);
    assert_eq!(root.get("axis").string(), "side_by_side");
    assert_eq!(root.get("ratio"), &Json::Number(0.5));
    assert_eq!(root.get("first").get("terminal").string(), "t4");
    assert_eq!(root.get("first").get("on_screen"), &Json::Bool(true));
    let stacked = root.get("second");
    assert_eq!(stacked.get("axis").string(), "stacked");
    assert_eq!(stacked.get("ratio"), &Json::Number(0.6));

    // A tab with a name, one without; a zoom; a side of other machines.
    assert_eq!(tabs[0].get("name").string(), "api");
    assert_eq!(tabs[0].get("keys").string(), "⌘1");
    assert_eq!(tabs[0].get("zoomed"), &Json::Null);
    assert_eq!(tabs[1].get("name"), &Json::Null);
    assert_eq!(tabs[1].get("zoomed").get("terminal").string(), "t12");
    assert_eq!(
        tabs[1].get("tree").get("first").get("on_screen"),
        &Json::Bool(false)
    );
    assert_eq!(tabs[2].get("keys"), &Json::Null);
    assert_eq!(
        tabs[2].get("tree").get("second"),
        &Json::Object(vec![("elsewhere".to_owned(), Json::Bool(true))])
    );
    assert_eq!(
        tabs[3].get("tree"),
        &Json::Object(vec![("elsewhere".to_owned(), Json::Bool(true))])
    );

    // The terminals, in the table's order, with kablo's names for what kablo names.
    let rows = parsed.get("terminals").items();
    let ids: Vec<&str> = rows.iter().map(|row| row.get("id").string()).collect();
    assert_eq!(
        ids,
        [
            "t4", "t7", "t9", "t12", "t13", "t14", "t15", "t30", "t41", "b-0a1b"
        ]
    );
    assert_eq!(
        rows[0].keys(),
        [
            "id",
            "state",
            "activity",
            "program",
            "directory",
            "cols",
            "rows",
            "exit",
            "secret",
            "label",
            "started",
            "front_started",
            "front_is_own",
            "output_at",
            "opened_by",
            "placed",
            "window",
            "tab"
        ]
    );
    let by_id = |id: &str| {
        rows.iter()
            .find(|row| row.get("id").string() == id)
            .expect(id)
    };
    assert_eq!(by_id("t4").get("activity").string(), "idle");
    assert_eq!(by_id("t4").get("state").string(), "running");
    assert_eq!(by_id("t4").get("directory").string(), "/home/ada/api");
    assert_eq!(by_id("t4").get("cols"), &Json::Number(120.0));
    assert_eq!(by_id("t4").get("front_is_own"), &Json::Bool(true));
    assert_eq!(by_id("t4").get("front_started"), &Json::Null);
    assert_eq!(
        by_id("t4").get("started"),
        &Json::Number((NOW - 3700) as f64)
    );
    assert_eq!(by_id("t4").get("placed").string(), "tab");
    assert_eq!(by_id("t4").get("window"), &Json::Number(1.0));
    assert_eq!(by_id("t4").get("tab"), &Json::Number(1.0));
    assert_eq!(by_id("t7").get("activity").string(), "running");
    assert_eq!(by_id("t9").get("activity").string(), "password_prompt");
    assert_eq!(by_id("t9").get("secret"), &Json::Bool(true));
    assert_eq!(by_id("t12").get("state").string(), "ended");
    assert_eq!(by_id("t12").get("activity").string(), "exited");
    assert_eq!(
        by_id("t12").get("exit"),
        &Json::Object(vec![("code".to_owned(), Json::Number(1.0))])
    );
    assert_eq!(
        by_id("t13").get("exit"),
        &Json::Object(vec![("signal".to_owned(), Json::Number(9.0))])
    );
    assert_eq!(by_id("t12").get("label").string(), "build");
    assert_eq!(by_id("t12").get("opened_by").string(), "t4");
    assert_eq!(by_id("t14").get("activity").string(), "broken");
    assert_eq!(by_id("t41").get("placed").string(), "list");
    assert_eq!(by_id("t41").get("window"), &Json::Null);
    assert_eq!(by_id("t30").get("window"), &Json::Number(2.0));
}

#[test]
fn the_json_of_a_small_scene_is_this_exactly() {
    let mut shell = status("t4", "zsh");
    shell.front_is_own = Some(true);
    shell.started = Some(100);
    shell.output_at = Some(160);
    shell.cwd = Some(b"/home/ada".to_vec());
    let scene = Scene {
        caller: Some("t4".to_owned()),
        typing: None,
        windows: Some(vec![Window {
            visible: true,
            tabs: vec![Tab {
                keys: Some("⌘1".to_owned()),
                name: None,
                selected: true,
                zoomed: None,
                tree: tree(
                    &pair(Axis::Vertical, 1.0 / 3.0, leaf(1), leaf(2)),
                    &[(1, "t4"), (2, "t5")],
                ),
            }],
        }]),
        terminals: vec![row(shell)],
    };
    let expected = "{\"caller\":\"t4\",\"typing\":null,\"windows\":[
{\"number\":1,\"visible\":true,\"tabs\":[{\"number\":1,\"keys\":\"⌘1\",\"name\":null,\"selected\":true,\"zoomed\":null,\"tree\":{\"axis\":\"stacked\",\"ratio\":0.333,\"first\":{\"terminal\":\"t4\",\"on_screen\":true},\"second\":{\"terminal\":\"t5\",\"on_screen\":true}}}]}
],\"terminals\":[
{\"id\":\"t4\",\"state\":\"running\",\"activity\":\"idle\",\"program\":\"zsh\",\"directory\":\"/home/ada\",\"cols\":120,\"rows\":30,\"exit\":null,\"secret\":false,\"label\":null,\"started\":100,\"front_started\":null,\"front_is_own\":true,\"output_at\":160,\"opened_by\":null,\"placed\":\"tab\",\"window\":1,\"tab\":1}
]}
";
    assert_eq!(scene.json(), expected);
    Json::parse(&scene.json());
    // Nothing at all.
    let nothing = Scene {
        caller: None,
        typing: None,
        windows: Some(Vec::new()),
        terminals: Vec::new(),
    };
    assert_eq!(
        nothing.json(),
        "{\"caller\":null,\"typing\":null,\"windows\":[],\"terminals\":[]}\n"
    );
}

#[test]
fn a_string_is_escaped_the_way_kablo_escapes_it() {
    let mut out = String::new();
    json_string(&mut out, "a\"b\\c\nd\re\tf\u{1}g\u{7f}é✓");
    assert_eq!(out, "\"a\\\"b\\\\c\\nd\\re\\tf\\u0001g\\u007fé✓\"");
    assert_eq!(Json::parse(&out).string(), "a\"b\\c\nd\re\tf\u{1}g\u{7f}é✓");
    for (ratio, text) in [
        (0.0, "0"),
        (1.0, "1"),
        (0.5, "0.5"),
        (0.25, "0.25"),
        (2.0 / 3.0, "0.667"),
        (7.0, "1"),
        (-1.0, "0"),
    ] {
        let mut out = String::new();
        json_ratio(&mut out, ratio);
        assert_eq!(out, text, "{ratio}");
    }
}
