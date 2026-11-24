//! A development copy's way to make a program's view fail on purpose: what the failures of a view
//! that closes bateri (a hard exit, a panic or an Objective-C exception on the main thread) are
//! tried with, in a copy that has the terminals of its own ([`crate::profile`], `make dev`).
//!
//! `BATERI_DEV_FAULT=<program>:<kind>[:<milliseconds>]` in the copy's environment (`open --env`):
//! the program is the name its view says it has (`*` is any), the kind is `exit` (the process
//! ends as a killed one does, as nothing catches: [`view_opened`], after the milliseconds),
//! `panic` (a Rust panic where the page is built) or `exception` (an Objective-C exception where
//! the keyboard is handed to the page); the last two are the places the page's boundary stands
//! ([`fires`]). **Only a copy whose profile is named `dev…` reads it:** the application itself
//! has no such door, whatever its environment says.

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
    /// A panic where the page is built.
    Panic,
    /// An Objective-C exception where the keyboard is handed to the page.
    Exception,
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

/// Whether the view of `program` is to fail as `kind` says, where the code that fails it stands.
fn fires(kind: Kind, program: Option<&str>) -> bool {
    plan().is_some_and(|plan| plan.kind == kind && matches(plan, program))
}

/// What a page's frame hands the pane to deliver (what the page asked of it: the keyboard, a
/// text, a copy), from the frame's own turn on the main thread — where the chain of the failure
/// that closed bateri on 2026-11-23 stood. A panic or an Objective-C exception is raised here
/// when the plan says one for this program.
pub(crate) fn page_delivered(program: Option<&str>) {
    if fires(Kind::Panic, program) {
        panic!("dev fault: a panic where a page's frame is delivered");
    }
    if fires(Kind::Exception, program) {
        eprintln!("bateri: dev fault: an Objective-C exception where a page's frame is delivered");
        let name = NSString::from_str("BateriDevFault");
        let reason = NSString::from_str("a fault the development copy was asked for");
        // SAFETY: raising is the point: the exception unwinds through Rust frames as AppKit's
        // own would and nothing catches it, which is what this copy is for.
        unsafe {
            NSException::exceptionWithName_reason_userInfo(&name, Some(&reason), None).raise();
        }
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

    #[test]
    fn nothing_fails_in_a_process_that_names_no_profile() {
        // The tests run unbundled: no `BateriProfile`, so no plan, whatever the environment says.
        assert!(plan().is_none());
        assert!(!fires(Kind::Panic, Some("textpen")));
    }
}
