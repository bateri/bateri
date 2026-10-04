//! bateri — application entry point.

#[cfg(test)]
mod bundle_assets;

use std::process::{Command, ExitCode};
use std::time::Instant;

fn main() -> ExitCode {
    // ssh's askpass: the same binary, started by our own master
    // connection with `BATERI_ASKPASS` in its environment. Before everything
    // else — the stamp, `launchctl`, AppKit — because only the answer may
    // reach standard output and a window-server session is not needed.
    if let Some(code) = bt_shell_macos::askpass() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // The remote shell integration's decision: `bateri ssh-argv -- …`,
    // asked by the local zsh's `ssh` function. Before the window-server check
    // for the same reason as askpass: only the wrapped argv may reach standard
    // output, and the caller may be any session.
    if let Some(code) = bt_shell_macos::ssh_argv() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // Its sibling: `bateri ssh-fell-back --rc N -- …`, asked after a
    // wrapped `ssh` ended — the same function, the same wire, the same reason.
    if let Some(code) = bt_shell_macos::ssh_fell_back() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // The focus query: `bateri focus [--pid P] bateri://tab/<UUID>`,
    // asked by an outside process at the moment of an event — the same
    // reasons: only the token line reaches standard output, any session.
    if let Some(code) = bt_shell_macos::focus() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // The update's holder: `bateri hold --fd N --dir D`, started by the
    // old bateri while it exits — before the window-server check, since it
    // has no GUI, and before the unknown-subcommand rule.
    if let Some(code) = bt_shell_macos::hold() {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    // An unknown subcommand must not open a window: a typo, or a
    // subcommand of a newer bateri asked of an older one, would otherwise
    // start the whole GUI. Arguments starting with `-` pass —
    // LaunchServices' `-psn_…` among them.
    if let Some(arg) = std::env::args_os().nth(1)
        && is_unknown_subcommand(&arg)
    {
        eprintln!("bateri: unknown subcommand {arg:?}");
        // EX_USAGE, beside the EX_CONFIG below.
        return ExitCode::from(64);
    }
    // **First line of the application.** The earlier the startup stamp is taken the more honest
    // it is: `has_aqua_session()` right below spawns a child process and that
    // is today a part of bateri's startup path. Had the stamp been taken after
    // it, `startup=` would silently have dropped that time.
    //
    // Still **not the process start**: dyld and Rust runtime setup are already
    // done before this stamp. This is the earliest point we have and the
    // number means "from main to the first completed frame".
    //
    // While the gate is closed the clock is **never** read: the `then`
    // closure runs only on `true`.
    let stats_since = std::env::var_os("BT_FRAME_STATS")
        .is_some()
        .then(Instant::now);
    // Headless environment (SSH, CI): AppKit cannot connect to the WindowServer
    // and fails with an unclear error. Skipping ≠ passing: exit explicitly
    // with 78 (EX_CONFIG).
    if !has_aqua_session() {
        // stdout: the same channel as the `frames=` line, `make smoke` reads it from one place.
        println!("SKIPPED: no Aqua session");
        return ExitCode::from(78);
    }
    // A malformed value must not silently turn into "no deadline": the smoke
    // hook either works or falls red.
    let run_seconds = match std::env::var("BT_RUN_SECONDS") {
        Ok(s) => match s.parse::<u64>() {
            Ok(n) => Some(n),
            Err(_) => {
                eprintln!("bateri: BT_RUN_SECONDS is not a number: {s:?}");
                return ExitCode::FAILURE;
            }
        },
        Err(_) => None,
    };
    // `BT_SCROLL_TEST` selects the load, `BT_FRAME_STATS` turns measurement on;
    // neither selects the **duration** and both are meaningless without one. A
    // load without a duration never ends; a measurement without a duration is
    // never reported (the report is printed only on the deadline path) and would
    // silently accumulate samples and discard them. Zero is also rejected: a
    // zero-second run measures nothing and would fall at the gate with
    // `glyphs=0`, sending the reader to a pipeline failure that does not exist.
    //
    // The env is read **only here** and goes deep into the code as a typed
    // field. The parsing half does not resemble `BT_RUN_SECONDS` and
    // need not: the **presence** of these two flags carries the meaning, not
    // the value — `BT_SCROLL_TEST=0` also selects the load.
    let scroll_test = std::env::var_os("BT_SCROLL_TEST").is_some();
    // `BT_JOURNAL` records every pane's journal in this process — the cost a
    // crash-proof screen puts on the reader thread, for `/measure`; a
    // journal nobody compacts or reads back has no use outside a timed run.
    let journal = std::env::var_os("BT_JOURNAL").is_some();
    for (name, asked) in [
        ("BT_SCROLL_TEST", scroll_test),
        ("BT_FRAME_STATS", stats_since.is_some()),
        ("BT_JOURNAL", journal),
    ] {
        if asked && !matches!(run_seconds, Some(n) if n > 0) {
            eprintln!("bateri: {name} needs a BT_RUN_SECONDS greater than zero");
            return ExitCode::FAILURE;
        }
    }
    // Backward compatibility: `BT_RUN_SECONDS` alone selects the smoke load as
    // before, so `make smoke` works completely unchanged.
    let run = run_seconds.map(|seconds| bt_shell_macos::Run {
        seconds,
        workload: if scroll_test {
            bt_shell_macos::Workload::Load
        } else {
            bt_shell_macos::Workload::Smoke
        },
        stats_since,
        journal,
    });
    match bt_shell_macos::run(bt_shell_macos::Options { run }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bateri: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Whether argv[1] — left after every subcommand above passed — is a
/// subcommand nobody knows: anything not starting with `-`.
fn is_unknown_subcommand(arg: &std::ffi::OsStr) -> bool {
    !arg.to_string_lossy().starts_with('-')
}

fn has_aqua_session() -> bool {
    Command::new("launchctl")
        .arg("managername")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "Aqua")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_unknown_subcommand;
    use std::ffi::OsStr;

    #[test]
    fn only_dashless_arguments_are_unknown_subcommands() {
        for unknown in ["nonsense", "focsu", "bateri://tab/x", ""] {
            assert!(is_unknown_subcommand(OsStr::new(unknown)), "{unknown:?}");
        }
        // LaunchServices' process serial number and any flag still open the GUI.
        for flag in ["-psn_0_12345", "--help", "-"] {
            assert!(!is_unknown_subcommand(OsStr::new(flag)), "{flag:?}");
        }
    }
}
