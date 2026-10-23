//! `demopen`: a made-up Docker host, and the smallest program that shows a view of its own.
//!
//! In a terminal that speaks pencere it opens the container list as a view of the terminal's
//! own (a WebAssembly module it carries, `containers.wasm`) and, once a second, tells it how
//! much CPU and memory each running container uses — the view does not ask again. In a terminal
//! that does not, and when asked for text, it prints the list. ⌃C ends it and the shell is where
//! it was.
//!
//! ```text
//! demopen            the view if the terminal shows one, the list as text if not
//! demopen --flood    the view, with the readings pushed as fast as the terminal takes them
//! demopen --text     the list as text, whatever the terminal is
//! demopen --json     the list as JSON, a line for each container
//! ```
//!
//! In the view, Shift-U sends this program a large payload; the program counts what it gets and
//! says how much when it leaves.
//!
//! The list and the numbers are the page's own (`bt_view::demo`): this program makes up nothing
//! the page does not know the names of. The protocol half (frames, bundle, token) is
//! `pencere`'s, written by the crate that reads it; the messages are `bateri-view-messages`'.

mod table;
mod terminal;
mod view;

use std::process::ExitCode;

/// The view's module: built by `make guest`, which builds it before this program.
const VIEW: &[u8] = include_bytes!("../../dist/containers.wasm");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => shown(false),
        ["--flood"] => shown(true),
        ["--text"] => {
            print!("{}", table::text());
            ExitCode::SUCCESS
        }
        ["--json"] => {
            print!("{}", table::json());
            ExitCode::SUCCESS
        }
        ["--help" | "-h"] => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(64)
        }
    }
}

/// The view if the terminal shows one, and the list as text if not.
fn shown(flood: bool) -> ExitCode {
    if terminal::is_terminal() {
        let (outcome, uploaded) = view::run(VIEW, flood);
        if uploaded > 0 {
            println!("demopen: the view sent {uploaded} bytes");
        }
        match outcome {
            view::Outcome::Ended => return ExitCode::SUCCESS,
            view::Outcome::Interrupted => return ExitCode::from(130),
            view::Outcome::NoView => {}
        }
    }
    print!("{}", table::text());
    ExitCode::SUCCESS
}

const USAGE: &str = "usage: demopen [--flood | --text | --json]\n\n  \
    (no option)  a view in a terminal that shows one, the list as text in one that does not\n  \
    --flood      the view, with the readings pushed as fast as the terminal takes them\n  \
    --text       the list as text\n  \
    --json       the list as JSON, a line for each container";
