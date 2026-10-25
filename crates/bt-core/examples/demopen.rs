//! `demopen`: the program the end-to-end tests of a pane run in it, to show a view of its own the
//! way a person's program does (the tests in `session.rs` start it as the pane's child, on a real
//! PTY, and play the terminal's side: the page and the sandbox it runs in are the test's).
//!
//! It is written with pencere's own program library (`pencere`, the crate of the program's side
//! of the protocol) and carries no page of its own: the bundle it opens is a stand-in the tests
//! can recognise — the header of a component, or a few lines of Lua that name bateri — because
//! what the tests look at is the conversation, the flow, the signals and the way out, not what a
//! page draws.
//!
//! ```text
//! demopen            a view of the kind `wasm`, readings pushed once a second
//! demopen --lua      the same, with a script as the view
//! demopen --lua-files  the same, with a script of two files: the page, and a script it requires
//! demopen --flood    the readings pushed as fast as the terminal takes them
//! demopen --stall N  the readings stop after N seconds (a Docker that hangs)
//! ```
//!
//! The view's `stop` call is not known (the library answers it with an error) and its `upload`
//! calls carry a large payload in pieces: the program counts what it gets and says how much
//! when it leaves, with how the view ended if it failed. ⌃C ends it as the protocol says.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use pencere::{
    Bundle, Call, EndedBy, File, Kind, NoViewReason, Options, Outcome, Subscription, View,
    ViewError,
};
use serde_json::{Value, json};

/// The first bytes of a WebAssembly component: what a pane holds and serves as the module.
const COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";

/// A script that says whose it is.
const SCRIPT: &[u8] =
    b"-- a page of bateri's tests\nreturn { render = function() return pencere.div() end }\n";

/// A page of two files, for `--lua-files`: this one, and `parts/box.luau`, which it requires.
const PAGE: &[u8] = b"-- a page of bateri's tests, of two files\nlocal box = require(\"./parts/box\")\nreturn { render = function()\n\treturn pencere.div():bg(\"raised1\"):p(8):child(pencere.text(\"two files\")):child(box())\nend }\n";
const BOX: &[u8] = b"return function() return pencere.div():size(20, 20):bg(\"accent\") end\n";

/// The version of the interface of the views the terminal draws itself that this is written
/// against.
const VERSION: &str = "1.0";

/// How often the made-up host reports.
const EVERY: Duration = Duration::from_secs(1);

/// The containers of the made-up host.
const CONTAINERS: [&str; 4] = ["api", "db", "cache", "toolbox"];

fn main() -> ExitCode {
    let mut flood = false;
    let mut stall = None;
    let mut lua = false;
    let mut files = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--flood" => flood = true,
            "--lua" => lua = true,
            "--lua-files" => {
                lua = true;
                files = true;
            }
            "--stall" => match args
                .next()
                .and_then(|seconds| seconds.parse::<f64>().ok())
                .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
            {
                Some(after) => stall = Some(after),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    let (kind, name, file) = if lua {
        (Kind::Lua, "containers.lua", SCRIPT)
    } else {
        (Kind::Wasm, "containers.wasm", COMPONENT)
    };
    let bundle = if files {
        Bundle::new(
            "page.lua",
            [
                File::new("page.lua", PAGE),
                File::new("parts/box.luau", BOX),
            ],
        )
    } else {
        Bundle::single(name, file)
    };
    let Ok(bundle) = bundle else {
        return ExitCode::from(66);
    };
    let upload = Arc::new(Upload::default());
    let counted = Arc::clone(&upload);
    let stalls_at = stall.map(|after| Instant::now() + after);
    let options = Options::new()
        .view(View::new(kind, &[VERSION], bundle))
        .program("demopen")
        .title("Containers")
        // The readings: the first is the view's at once, the next ones are a second apart.
        .topic("stats", move |_params, subscription| {
            let feed = subscription.clone();
            std::thread::spawn(move || report(&feed, flood, stalls_at));
            Ok(())
        })
        // The view sends a large payload in pieces; the last one asks how much there was.
        .call("upload", move |params, call| counted.take(&params, call))
        // The terminal shows no view: the list as text. A refusal or a failure has its words.
        .fallback(|why| {
            if let Some(words) = &why.message {
                say(&format!("demopen: the view failed: {}\n", printable(words)));
            } else if why.reason == NoViewReason::Refused {
                say("demopen: the terminal refused the view\n");
            }
            say(&format!("{}\n", CONTAINERS.join("\n")));
        });
    let outcome = match pencere::serve(options) {
        Ok(outcome) => outcome,
        Err(why) => {
            say(&format!("demopen: {why}\n"));
            return ExitCode::from(70);
        }
    };
    // The terminal is the shell's again: this is where the tests read what happened.
    let bytes = upload.total();
    if bytes > 0 {
        say(&format!("demopen: the view sent {bytes} bytes\n"));
    }
    if let Outcome::Closed(closed) = &outcome
        && closed.by == EndedBy::Error
        && let Some(words) = &closed.message
    {
        say(&format!("demopen: the view failed: {}\n", printable(words)));
    }
    ExitCode::from(outcome.shell_status())
}

fn usage() -> ExitCode {
    say("usage: demopen [--lua | --lua-files] [--flood] [--stall SECONDS]\n");
    ExitCode::from(64)
}

/// The readings of the made-up host, pushed until the view stops listening. With `flood` as fast
/// as the terminal takes them; with a time to `stall_at`, until then.
fn report(feed: &Subscription, flood: bool, stall_at: Option<Instant>) {
    let mut tick = 0u32;
    while !feed.is_cancelled() {
        if stall_at.is_some_and(|at| Instant::now() >= at) {
            // A Docker that hangs: nothing more is said, and the subscription stays open.
            while !feed.sleep(Duration::from_millis(100)) {}
            return;
        }
        let readings: Vec<Value> = CONTAINERS
            .iter()
            .enumerate()
            .map(|(at, name)| {
                let cpu = f64::from((tick + at as u32 * 7) % 100);
                json!({ "name": name, "cpu": cpu, "memory": 40.0 + cpu / 4.0, "share": cpu / 100.0 })
            })
            .collect();
        tick = tick.wrapping_add(1);
        if !feed.push(json!({ "readings": readings })) {
            return;
        }
        // `push` waits for the terminal when it is behind, which is the pace of a flood.
        if !flood && feed.sleep(EVERY) {
            return;
        }
    }
}

/// What the pieces of the payload the view sends have added up to. The pieces are calls, and calls
/// run at the same time: the last one waits for the others before it says how much there was.
#[derive(Default)]
struct Upload {
    /// The size of the data of each piece, by its number.
    pieces: Mutex<BTreeMap<u64, u64>>,
    bytes: AtomicU64,
}

impl Upload {
    fn take(&self, params: &Value, call: &Call) -> Result<Value, ViewError> {
        let data = params
            .get("data")
            .and_then(Value::as_str)
            .map_or(0, str::len) as u64;
        let seq = params.get("seq").and_then(Value::as_u64).unwrap_or(0);
        let last = params.get("last").and_then(Value::as_bool) == Some(true);
        self.add(seq, data);
        if !last {
            return Ok(json!({}));
        }
        // The pieces are numbered from 1, and the last one's number is how many there are.
        while self.count() < seq {
            if call.sleep(Duration::from_millis(5)) {
                return Ok(json!({}));
            }
        }
        Ok(json!({ "bytes": self.total() }))
    }

    fn add(&self, seq: u64, size: u64) {
        let Ok(mut pieces) = self.pieces.lock() else {
            return;
        };
        if pieces.insert(seq, size).is_none() {
            self.bytes.fetch_add(size, Ordering::SeqCst);
        }
    }

    fn count(&self) -> u64 {
        self.pieces.lock().map_or(0, |pieces| pieces.len() as u64)
    }

    fn total(&self) -> u64 {
        self.bytes.load(Ordering::SeqCst)
    }
}

/// Writes to the terminal; one that has gone away is nobody to tell (`print!` would panic).
fn say(text: &str) {
    let _ = std::io::stdout().write_all(text.as_bytes());
    let _ = std::io::stdout().flush();
}

/// Words somebody else wrote, fit for a terminal that is going to print them.
fn printable(words: &str) -> String {
    words.chars().filter(|c| !c.is_control()).collect()
}
