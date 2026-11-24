//! A development copy's way to make a program's view fail on purpose: what the failures of a view
//! that closes bateri (a hard exit, a panic or an Objective-C exception on the main thread) are
//! tried with, in a copy that has the terminals of its own ([`crate::profile`], `make dev`).
//!
//! `BATERI_DEV_FAULT=<program>:<kind>[:<milliseconds>]` in the copy's environment (`open --env`):
//! the program is the name its view says it has (`*` is any), the kind is
//!
//! - `exit`: the process ends as a killed one does, as nothing catches ([`view_opened`], after the
//!   milliseconds) — the failure the page's boundary cannot meet, and the one the loop is tried
//!   with;
//! - `panic`: a Rust panic where the page's frame is built ([`page_delivered`]);
//! - `exception`: an Objective-C exception raised there (objc2 makes it a panic where it is
//!   raised, `catch-all`);
//! - `keyboard`: the same exception where the keyboard is handed to a field of the page
//!   ([`keyboard_handed`]), the chain of the failure that closed bateri on 2026-11-23.
//!
//! **Only a copy whose profile is named `dev…` reads it:** the application itself has no such
//! door, whatever its environment says.

use std::sync::OnceLock;
use std::time::Duration;

use objc2_foundation::{NSException, NSString};

/// The variable the copy reads.
const VARIABLE: &str = "BATERI_DEV_FAULT";

/// How the view fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// The process ends at once, with nothing run: a crash nothing catches.
    Exit,
    /// A panic where the page's frame is built.
    Panic,
    /// An Objective-C exception where the page's frame is built.
    Exception,
    /// An Objective-C exception where the keyboard is handed to a field of the page.
    Keyboard,
}

#[derive(Debug, PartialEq, Eq)]
struct Plan {
    program: String,
    kind: Kind,
    delay: Duration,
}

/// `program:kind[:milliseconds]`; `None` for anything else.
fn parse(text: &str) -> Option<Plan> {
    let mut parts = text.trim().splitn(3, ':');
    let program = parts.next().filter(|program| !program.is_empty())?;
    let kind = match parts.next()? {
        "exit" => Kind::Exit,
        "panic" => Kind::Panic,
        "exception" => Kind::Exception,
        "keyboard" => Kind::Keyboard,
        _ => return None,
    };
    let delay = match parts.next() {
        None => Duration::ZERO,
        Some(millis) => Duration::from_millis(millis.parse().ok()?),
    };
    Some(Plan {
        program: program.to_owned(),
        kind,
        delay,
    })
}

/// Whether a view of `program` fails the way `plan` says.
fn matches(plan: &Plan, program: Option<&str>) -> bool {
    plan.program == "*" || program == Some(plan.program.as_str())
}

/// The plan of this copy: none for the application and for a copy that was asked for none.
fn plan() -> Option<&'static Plan> {
    static PLAN: OnceLock<Option<Plan>> = OnceLock::new();
    PLAN.get_or_init(|| {
        if !crate::profile().is_some_and(|profile| profile.starts_with("dev")) {
            return None;
        }
        parse(&std::env::var(VARIABLE).ok()?)
    })
    .as_ref()
}

/// Whether this copy was told to fail a view at all: nothing is hung on the frames otherwise.
pub(crate) fn armed() -> bool {
    plan().is_some_and(|plan| plan.kind != Kind::Exit)
}

/// Whether the view of `program` is to fail as `kind` says, where the code that fails it stands.
fn fires(kind: Kind, program: Option<&str>) -> bool {
    plan().is_some_and(|plan| plan.kind == kind && matches(plan, program))
}

/// The page's frame is being built for the view of `program`, on the display link's turn on the
/// main thread: a panic or an Objective-C exception is raised here when the plan says one.
pub(crate) fn page_delivered(program: Option<&str>) {
    if fires(Kind::Panic, program) {
        panic!("dev fault: a panic where a page's frame is built");
    }
    if fires(Kind::Exception, program) {
        eprintln!("bateri: dev fault: an Objective-C exception where a page's frame is built");
        raise();
    }
}

/// The keyboard is being handed to a field of the page of `program`: the exception of the chain of
/// 2026-11-23, raised in `makeFirstResponder`'s place, when the plan says one.
pub(crate) fn keyboard_handed(program: Option<&str>) {
    if fires(Kind::Keyboard, program) {
        eprintln!("bateri: dev fault: an Objective-C exception where the keyboard is handed over");
        raise();
    }
}

/// An `NSException`, raised as AppKit's own are: it unwinds through whatever Rust frames are on
/// the stack, unless the message send that raises it was one objc2 wraps (`catch-all`).
fn raise() {
    let name = NSString::from_str("BateriDevFault");
    let reason = NSString::from_str("a fault the development copy was asked for");
    // SAFETY: raising is the point; it is the exception a failing page would raise.
    unsafe {
        NSException::exceptionWithName_reason_userInfo(&name, Some(&reason), None).raise();
    }
}

/// A view of `program` is open: the hard exit comes after its delay, from a thread of its own
/// (the main thread goes on as if nothing were wrong, as it does when a page fails).
pub(crate) fn view_opened(program: Option<&str>) {
    let Some(plan) = plan() else {
        return;
    };
    if plan.kind != Kind::Exit || !matches(plan, program) {
        return;
    }
    let delay = plan.delay;
    eprintln!("bateri: dev fault: the process ends {delay:?} after a view of {program:?} opened");
    let _ = std::thread::Builder::new()
        .name("bateri dev fault".into())
        .spawn(move || {
            std::thread::sleep(delay);
            // SAFETY: `kill` on the process's own id takes no memory; the signal cannot be
            // caught, so nothing runs after it and no crash report is made for the person.
            unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fault_names_the_program_the_kind_and_a_delay() {
        assert_eq!(
            parse("linearpen:exit:22000"),
            Some(Plan {
                program: "linearpen".into(),
                kind: Kind::Exit,
                delay: Duration::from_millis(22000)
            })
        );
        assert_eq!(parse("*:panic").map(|plan| plan.kind), Some(Kind::Panic));
        assert_eq!(
            parse("pickpen:keyboard").map(|plan| plan.kind),
            Some(Kind::Keyboard)
        );
        assert_eq!(
            parse(" demopen:exception ").map(|plan| plan.delay),
            Some(Duration::ZERO)
        );
        for bad in ["", "exit", "x:crash", ":exit", "x:exit:soon", "x:exit:-1"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_fault_matches_the_program_it_names_or_every_one() {
        let named = parse("textpen:exit").expect("plan");
        assert!(matches(&named, Some("textpen")));
        assert!(!matches(&named, Some("linearpen")));
        assert!(!matches(&named, None));
        let any = parse("*:exit").expect("plan");
        assert!(matches(&any, None) && matches(&any, Some("anything")));
    }

    /// The boundary of a page meets what AppKit raises because objc2 makes it a panic where a
    /// message send raises it (`catch-all`): were it not on, the exception would unwind through
    /// Rust frames, which Rust cannot catch, and the process would end here.
    #[test]
    fn an_exception_raised_under_a_message_send_is_a_panic_a_boundary_catches() {
        let caught = std::panic::catch_unwind(raise);
        assert!(caught.is_err());
        // The same where Rust frames lie between the raise and the catch, as in a page's frame.
        fn deep(depth: u32) {
            if depth == 0 {
                raise();
            } else {
                deep(depth - 1);
            }
        }
        assert!(std::panic::catch_unwind(|| deep(5)).is_err());
    }

    #[test]
    fn nothing_fails_in_a_process_that_names_no_profile() {
        // The tests run unbundled: no `BateriProfile`, so no plan, whatever the environment says.
        assert!(plan().is_none());
        assert!(!fires(Kind::Panic, Some("textpen")));
    }
}
