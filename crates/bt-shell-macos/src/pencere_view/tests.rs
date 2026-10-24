//! The pure parts of a program's view: what its web view may reach, the headers
//! and the shell page it is given, and the deadlines.

use std::sync::Arc;
use std::time::Duration;

use bt_core::pencere::State;
use bt_shell_common::view_strip;
use objc2_app_kit::NSEventModifierFlags;
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::files::Pending;
use super::serve::{
    self, HANDLER, LOADED_HANDLER, Loads, Page, Route, SANDBOX, Served, css_color, deliver_script,
    ground_script, js_string, route,
};
use super::still::{self, Capture, PATIENCE, Shared, Still};
use super::wait::{OFFERED, OPENING, Waits};
use super::{INTERRUPT, QUIT, SUSPEND, modifiers_of, signal_of, strip_area, view_area};

const INSTANCE: &str = "v-0123456789abcdef0123456789abcdef";
const POLICY: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; connect-src 'self' https://api.example;";

fn url(path: &str) -> String {
    format!("bateri-view://{INSTANCE}/{path}")
}

fn bundle_page() -> Page {
    Page::new(
        INSTANCE.to_owned(),
        "index.html".to_owned(),
        POLICY.to_owned(),
        "camera; microphone",
        "#101010",
    )
}

/// A file service holding the files of a bundle, as the crate's gives them.
fn files(path: &str) -> Option<Served> {
    match path {
        "index.html" => Some(("text/html".to_owned(), Arc::from(&b"<html></html>"[..]))),
        "app.js" => Some((
            "text/javascript".to_owned(),
            Arc::from(&b"console.log(1)"[..]),
        )),
        "dir/style.css" => Some(("text/css".to_owned(), Arc::from(&b"body{}"[..]))),
        _ => None,
    }
}

// ---- what a URL names -------------------------------------------------------

#[test]
fn the_shell_is_the_one_page_at_the_roots_of_its_own_host() {
    for shell in [
        "bateri-view://shell/",
        "bateri-view://shell",
        "BATERI-VIEW://Shell/",
    ] {
        assert_eq!(route(INSTANCE, shell), Route::Shell, "{shell}");
    }
    assert_eq!(Page::shell_url(), "bateri-view://shell/");
    for other in [
        "bateri-view://shell/index.html",
        "bateri-view://shell:80/",
        "bateri-view://user@shell/",
    ] {
        assert_eq!(route(INSTANCE, other), Route::Refused, "{other}");
    }
}

#[test]
fn a_path_of_the_view_is_taken_bare_and_decoded() {
    assert_eq!(
        route(INSTANCE, &url("index.html")),
        Route::View("index.html".into())
    );
    assert_eq!(
        route(INSTANCE, &url("dir/style.css?v=2#top")),
        Route::View("dir/style.css".into()),
        "a query and a fragment are not the path"
    );
    assert_eq!(
        route(INSTANCE, &url("a%20b/%C3%BC.js")),
        Route::View("a b/\u{fc}.js".into())
    );
}

#[test]
fn a_foreign_instance_a_foreign_scheme_and_a_name_that_is_not_a_path_are_refused() {
    let other = "bateri-view://v-ffffffffffffffffffffffffffffffff/index.html";
    for refused in [
        other,
        "https://example.com/index.html",
        "file:///etc/passwd",
        "bateri-view:/index.html",
        "bateri-view://",
        "javascript:alert(1)",
        "",
    ] {
        assert_eq!(route(INSTANCE, refused), Route::Refused, "{refused}");
    }
    for refused in [
        url(""),
        url("../index.html"),
        url("dir/../index.html"),
        url("%2e%2e/index.html"),
        url("%2E%2E%2Findex.html"),
        url("./index.html"),
        url("dir//index.html"),
        url("dir/"),
        url("a%5Cb"),
        url("a%00b"),
        url("%ff"),
        url("%zz"),
        url("%4"),
    ] {
        assert_eq!(route(INSTANCE, &refused), Route::Refused, "{refused}");
    }
}

// ---- what it is answered with -----------------------------------------------

#[test]
fn a_file_of_the_bundle_is_served_with_its_type_and_the_crates_policy() {
    let page = bundle_page();
    for (path, media, body) in [
        ("index.html", "text/html", &b"<html></html>"[..]),
        ("app.js", "text/javascript", &b"console.log(1)"[..]),
        ("dir/style.css", "text/css", &b"body{}"[..]),
    ] {
        let reply = page.reply(&url(path), &files);
        assert_eq!(reply.status, 200, "{path}");
        assert_eq!(&*reply.body, body, "{path}");
        assert_eq!(reply.header("Content-Type"), Some(media), "{path}");
        assert_eq!(
            reply.header("Content-Security-Policy"),
            Some(POLICY),
            "the crate's policy, to the byte, on {path}"
        );
        assert_eq!(
            reply.header("Content-Length"),
            Some(body.len().to_string().as_str())
        );
        assert_eq!(reply.header("X-Content-Type-Options"), Some("nosniff"));
        assert_eq!(reply.header("Cache-Control"), Some("no-store"));
    }
}

/// Specification §12.1: the policy is the `Content-Security-Policy` header of the response to
/// every file served for the page, the entry included — a `<meta>` element is the page's own
/// writing and stands for nothing. So the policy is on every answer, the bytes of a file are
/// the program's untouched (no element is added to its entry to carry one), and the shell page
/// carries its own as a header too.
#[test]
fn the_policy_is_a_header_of_every_answer_and_never_an_element_of_a_page() {
    let page = bundle_page();
    for path in ["index.html", "app.js", "dir/style.css"] {
        let reply = page.reply(&url(path), &files);
        assert_eq!(reply.status, 200, "{path}");
        assert_eq!(
            reply.header("Content-Security-Policy"),
            Some(POLICY),
            "{path}"
        );
        let given = files(path).expect("a file of the bundle").1;
        assert_eq!(reply.body, given, "{path} is served as the program sent it");
    }
    let shell = page.reply(&Page::shell_url(), &files);
    assert!(shell.header("Content-Security-Policy").is_some());
    let text = shell_text(&page);
    assert!(
        !text.to_ascii_lowercase().contains("http-equiv"),
        "the shell's policy is its header: {text}"
    );
}

#[test]
fn what_the_manifest_does_not_name_is_a_404_and_so_is_every_other_origin() {
    let page = bundle_page();
    for missing in [
        url("secret.txt"),
        url("dir/other.css"),
        url("../index.html"),
        url("%2e%2e/index.html"),
        "bateri-view://v-ffffffffffffffffffffffffffffffff/index.html".to_owned(),
        "https://example.com/".to_owned(),
    ] {
        let reply = page.reply(&missing, &files);
        assert_eq!(reply.status, 404, "{missing}");
        assert!(reply.body.is_empty(), "{missing}");
        assert!(
            reply.header("Content-Security-Policy").is_some(),
            "even a refusal carries a policy"
        );
    }
}

#[test]
fn the_shell_frames_the_views_origin_and_nothing_else() {
    let page = bundle_page();
    let reply = page.reply(&Page::shell_url(), &files);
    assert_eq!(reply.status, 200);
    assert_eq!(
        reply.header("Content-Type"),
        Some("text/html; charset=utf-8")
    );
    let policy = reply.header("Content-Security-Policy").expect("a policy");
    assert!(policy.contains("default-src 'none'"), "{policy}");
    assert!(
        policy.contains(&format!("frame-src bateri-view://{INSTANCE};")),
        "{policy}"
    );
    assert!(policy.contains("script-src 'unsafe-inline'"), "{policy}");
    assert!(!policy.contains("connect-src"), "no fetch: default is none");
    assert!(policy.contains("form-action 'none'"), "{policy}");
    assert_eq!(
        reply.header("Content-Length"),
        Some(reply.body.len().to_string().as_str())
    );
}

// ---- the shell page ---------------------------------------------------------

fn shell_text(page: &Page) -> String {
    String::from_utf8(page.reply(&Page::shell_url(), &files).body.to_vec()).expect("text")
}

#[test]
fn the_frame_gets_the_views_url_the_crates_allow_and_the_fixed_sandbox() {
    let text = shell_text(&bundle_page());
    assert!(
        text.contains(&format!(
            "<iframe id=\"view\" src=\"{}\" sandbox=\"{SANDBOX}\" allow=\"camera; microphone\"></iframe>",
            url("index.html")
        )),
        "{text}"
    );
    assert!(text.contains("background: #101010;"), "{text}");
}

#[test]
fn the_sandbox_lets_the_view_run_and_leave_nothing_of_the_frame() {
    let tokens: Vec<&str> = SANDBOX.split(' ').collect();
    assert!(tokens.contains(&"allow-scripts"));
    assert!(
        tokens.contains(&"allow-same-origin"),
        "its own origin, which is not the shell's"
    );
    for forbidden in [
        "allow-top-navigation",
        "allow-top-navigation-by-user-activation",
        "allow-popups",
        "allow-popups-to-escape-sandbox",
        "allow-modals",
        "allow-downloads",
        "allow-pointer-lock",
        "allow-presentation",
        "allow-orientation-lock",
    ] {
        assert!(!tokens.contains(&forbidden), "{forbidden}");
    }
}

#[test]
fn what_the_frame_is_given_cannot_end_its_attribute() {
    let page = Page::new(
        INSTANCE.to_owned(),
        "a\"><script>x()</script>.html".to_owned(),
        POLICY.to_owned(),
        "camera\" onload=\"x()",
        "#000000\"><script>",
    );
    let text = shell_text(&page);
    assert!(
        text.contains("allow=\"camera&quot; onload=&quot;x()\""),
        "{text}"
    );
    // The entry's name is a path: escaped for the URL, then for the attribute.
    assert!(!text.contains("a\"><script>"), "{text}");
    assert!(
        text.contains("a%22%3E%3Cscript%3Ex%28%29%3C/script%3E.html"),
        "{text}"
    );
    assert!(!text.contains("<script>x()"), "{text}");
    assert_eq!(
        text.matches("<script>").count(),
        1,
        "the shell's own script only: {text}"
    );
}

#[test]
fn the_shell_takes_a_message_only_from_its_frames_window() {
    let text = shell_text(&bundle_page());
    assert!(
        text.contains("event.source !== frame.contentWindow"),
        "{text}"
    );
    assert!(text.contains("JSON.stringify(event.data)"), "{text}");
    assert!(text.contains("JSON.parse(text)"), "{text}");
    assert!(
        text.contains(&format!("handlers.{HANDLER};")),
        "the handler every message goes to: {text}"
    );
}

#[test]
fn the_shell_tells_the_application_when_the_frame_has_loaded_and_nothing_else_does() {
    let text = shell_text(&bundle_page());
    // On the frame's own load, on a handler of its own: the view's messages go to the other.
    assert!(
        text.contains(&format!("handlers.{LOADED_HANDLER};")),
        "{text}"
    );
    assert!(
        text.contains("frame.addEventListener(\"load\""),
        "the frame's load: {text}"
    );
    assert!(text.contains("loaded.postMessage(\"loaded\")"), "{text}");
    assert_ne!(HANDLER, LOADED_HANDLER);
    // The only call that posts what the view sent is the message listener's.
    assert_eq!(text.matches("host.postMessage(").count(), 1, "{text}");
}

/// The page's first `context` is not a call of the bridge's own: it is one more thing the page
/// is told and goes in their order, taken where the rest of them are (`drain_views`). A source
/// scan, because the bridge needs a window server.
#[test]
fn the_first_context_of_a_page_goes_in_the_order_of_what_the_page_is_told() {
    let source = include_str!("../pencere_view.rs");
    let at = source
        .find("if name.to_string() == serve::LOADED_HANDLER")
        .expect("the loaded handler");
    let body = &source[at..at + source[at..].find("return;").expect("its end")];
    assert!(body.contains("session.view_page_loaded()"), "{body}");
    assert!(!body.contains("evaluateJavaScript"), "{body}");
    // The one call of the shell page's script that hands the page a message is `deliver`'s.
    assert_eq!(source.matches("serve::deliver_script(").count(), 1);
    assert!(!source.contains("view_page_context"));
}

#[test]
fn a_css_colour_is_the_themes_bytes() {
    assert_eq!(css_color([0, 0, 0]), "#000000");
    assert_eq!(css_color([26, 28, 33]), "#1a1c21");
    assert_eq!(css_color([255, 255, 255]), "#ffffff");
}

// ---- what is passed to the shell's script -----------------------------------

#[test]
fn a_message_is_a_string_in_the_script_and_never_its_code() {
    assert_eq!(js_string("plain"), "\"plain\"");
    assert_eq!(js_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
    assert_eq!(js_string("a\nb\rc\td"), "\"a\\nb\\rc\\td\"");
    assert_eq!(js_string("\u{0}\u{1f}"), "\"\\u0000\\u001f\"");
    assert_eq!(js_string("\u{2028}\u{2029}"), "\"\\u2028\\u2029\"");
    assert_eq!(
        js_string("\u{1f600}"),
        "\"\u{1f600}\"",
        "printable text is as it is"
    );
    let hostile = "\");alert(1);(\"";
    let script = deliver_script(hostile);
    assert_eq!(script, "window.pencere.deliver(\"\\\");alert(1);(\\\"\")");
    // The literal is one argument: unescaped quotes would end it.
    let body = script
        .strip_prefix("window.pencere.deliver(")
        .and_then(|rest| rest.strip_suffix(')'))
        .expect("one call");
    let mut escaped = false;
    let mut quotes = 0;
    for c in body.chars() {
        match (escaped, c) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => quotes += 1,
            _ => {}
        }
    }
    assert_eq!(quotes, 2, "{body}");
    assert_eq!(
        ground_script("#102030"),
        "window.pencere.ground(\"#102030\")"
    );
}

// ---- what the web view may navigate to --------------------------------------

#[test]
fn the_shell_loads_once_as_the_top_document_and_nothing_else_does() {
    let page = bundle_page();
    let shell = Page::shell_url();
    let mut loads = Loads::default();
    assert!(
        !page.allows(&url("index.html"), true, &mut loads),
        "the view is not a top document"
    );
    assert!(!page.allows("https://example.com/", true, &mut loads));
    assert!(!page.allows("file:///etc/passwd", true, &mut loads));
    assert!(!page.allows("about:blank", true, &mut loads));
    assert_eq!(loads, Loads::default(), "a refusal is not a load");
    assert!(page.allows(&shell, true, &mut loads));
    assert!(
        !page.allows(&shell, true, &mut loads),
        "a reload is the shell loaded again"
    );
    assert!(!page.allows(&url("index.html"), true, &mut loads));
}

#[test]
fn the_views_frame_loads_its_entry_before_anything_else_and_nothing_else() {
    let page = bundle_page();
    let mut loads = Loads::default();
    assert!(
        !page.allows(&Page::shell_url(), false, &mut loads),
        "the shell inside the shell"
    );
    assert!(!page.allows("https://example.com/", false, &mut loads));
    assert!(!page.allows("http://localhost:8080/", false, &mut loads));
    assert!(!page.allows(
        "bateri-view://v-ffffffffffffffffffffffffffffffff/index.html",
        false,
        &mut loads
    ));
    assert!(!page.allows(&url("../index.html"), false, &mut loads));
    assert_eq!(loads, Loads::default(), "a refusal is not a load");
    // A blank frame has no origin to leave, and it is not the entry.
    assert!(page.allows("about:blank", false, &mut loads));
    assert!(!loads.entry);
    assert!(page.allows(&url("index.html"), false, &mut loads));
    assert!(loads.entry && !loads.shell);
}

/// Specification §12.3: after the load of its entry, every navigation of the web view is
/// cancelled, whatever starts it — a link, a change of `location`, a form, a reload — and the
/// bundle's own files are no exception.
#[test]
fn after_the_entry_has_loaded_nothing_is_navigated_to_whatever_starts_it() {
    let page = bundle_page();
    let mut loads = Loads::default();
    assert!(page.allows(&Page::shell_url(), true, &mut loads));
    assert!(page.allows(&url("index.html"), false, &mut loads));
    for destination in [
        // A link to the entry again, a reload and a form posted to the page itself.
        url("index.html"),
        url("index.html#section"),
        url("index.html?q=1"),
        // A link, a `location` and a form that name another file of the bundle.
        url("dir/other.html"),
        url("app.js"),
        // A blank page, the shell, and everything that is not the view's.
        "about:blank".to_owned(),
        Page::shell_url(),
        "https://example.com/".to_owned(),
        "file:///etc/passwd".to_owned(),
        "javascript:alert(1)".to_owned(),
    ] {
        assert!(
            !page.allows(&destination, false, &mut loads),
            "{destination}"
        );
        assert!(
            !page.allows(&destination, true, &mut loads),
            "{destination}"
        );
    }
    assert_eq!(
        loads,
        Loads {
            shell: true,
            entry: true
        },
        "nothing that was cancelled is recorded as a load"
    );
}

/// The decision is the bridge's one question, asked of every navigation, and the shell's
/// word that its frame has loaded is not what the lock hangs on: a navigation that comes
/// before the load event (a script that redirects as it parses) is a navigation too.
#[test]
fn the_bridge_asks_the_one_question_of_every_navigation_and_keeps_its_answers() {
    let source = include_str!("../pencere_view.rs");
    let at = source
        .find("    fn allows(&self, action: &WKNavigationAction)")
        .expect("the bridge's allows");
    let body = source[at..].split("\n    }\n").next().expect("its end");
    assert!(
        body.contains("ivars.page.allows(&url, top, &mut loads)"),
        "{body}"
    );
    assert!(body.contains("ivars.loads.set(loads)"), "{body}");
    // A navigation with no target frame is a new window's, and it is refused.
    assert!(body.contains("let Some(target) = target else"), "{body}");
    let decide = source
        .find("fn decide(")
        .map(|at| source[at..].split("\n        }\n").next().expect("its end"))
        .expect("decide");
    assert!(decide.contains("self.allows(action)"), "{decide}");
    assert!(
        decide.contains("WKNavigationActionPolicy::Cancel"),
        "{decide}"
    );
    // The one place that lets anything load is that question: nothing else answers `Allow`.
    assert_eq!(source.matches("WKNavigationActionPolicy::Allow").count(), 1);
}

#[test]
fn an_origin_is_random_hex_that_is_never_the_shells_name() {
    let name = serve::instance([0xab; 16]);
    assert_eq!(name, "v-abababababababababababababababab");
    assert_ne!(name, "shell");
    assert_ne!(serve::instance([0; 16]), serve::instance([1; 16]));
}

// ---- the key the application keeps ------------------------------------------

#[test]
fn the_ttys_signal_characters_alone_are_the_programs() {
    let control = NSEventModifierFlags::Control;
    // ⌃C, ⌃\ and ⌃Z, each as the byte the tty turns into its signal (specification §11.3).
    for (character, byte) in [("\u{3}", INTERRUPT), ("\u{1c}", QUIT), ("\u{1a}", SUSPEND)] {
        assert_eq!(signal_of(control, Some(character)), Some(byte));
        // Caps Lock and the numeric pad's flags are not modifiers that change it.
        assert_eq!(
            signal_of(control | NSEventModifierFlags::CapsLock, Some(character)),
            Some(byte)
        );
        for flags in [
            NSEventModifierFlags::empty(),
            control | NSEventModifierFlags::Shift,
            control | NSEventModifierFlags::Command,
            control | NSEventModifierFlags::Option,
            NSEventModifierFlags::Command,
        ] {
            assert_eq!(signal_of(flags, Some(character)), None, "{flags:?}");
        }
    }
    assert_eq!(
        (INTERRUPT, QUIT, SUSPEND),
        (0x03, 0x1c, 0x1a),
        "the bytes the tty is set to take unless the program changed them"
    );
    assert_eq!(
        signal_of(control, Some("c")),
        None,
        "a letter is the view's"
    );
    assert_eq!(signal_of(control, Some("z")), None);
    assert_eq!(signal_of(control, Some("\\")), None);
    assert_eq!(
        signal_of(control, Some("\u{4}")),
        None,
        "⌃D is not a signal"
    );
    assert_eq!(signal_of(control, None), None);
    assert_eq!(signal_of(NSEventModifierFlags::empty(), Some("c")), None);
}

/// Both kinds of view send the three characters by the one function and the one writer: the
/// web view's `keyDown:` and the page's key, and neither names a byte of its own.
#[test]
fn both_kinds_of_view_send_a_signal_character_the_one_way() {
    let view = include_str!("../pencere_view.rs");
    let at = view.find("fn key_down(").expect("the web view's keyDown:");
    let key_down = view[at..].split("\n        }\n").next().expect("its end");
    assert!(key_down.contains("signal_of("), "{key_down}");
    assert!(
        key_down.contains("pane.signal_program(character)"),
        "{key_down}"
    );
    let pane = include_str!("../pane.rs");
    let pane = &pane[..pane.find("#[cfg(test)]").unwrap_or(pane.len())];
    let at = pane.find("pub(crate) fn page_key(").expect("page_key");
    let page_key = pane[at..].split("\n    }\n").next().expect("its end");
    assert!(page_key.contains("pencere_view::signal_of("), "{page_key}");
    // The three characters are written by one function, which is the gate of the terminal's
    // own keys; nothing else in either file writes a signal byte.
    assert_eq!(pane.matches("session.write(&[character])").count(), 1);
    assert!(!pane.contains("INTERRUPT]"), "no byte is written by name");
}

// ---- the deadlines ----------------------------------------------------------

#[test]
fn a_wait_is_set_once_for_the_state_that_waits_and_for_no_other() {
    let mut waits = Waits::default();
    for quiet in [State::Idle, State::Open, State::Ended] {
        assert_eq!(waits.observe(Some(quiet)), None, "{quiet:?}");
    }
    assert_eq!(waits.observe(None), None, "a pane that hosts no views");

    let (opening, delay) = waits.observe(Some(State::Opening)).expect("an open waits");
    assert_eq!(delay, OPENING);
    assert_eq!(
        waits.observe(Some(State::Opening)),
        None,
        "news in the same state does not push the time out"
    );
    let (offered, delay) = waits.observe(Some(State::Offered)).expect("an offer waits");
    assert_eq!(delay, OFFERED);
    assert_ne!(opening, offered);
    assert_eq!(waits.observe(Some(State::Offered)), None);
}

#[test]
fn a_wait_that_is_not_the_one_set_is_nothing_when_it_wakes() {
    let mut waits = Waits::default();
    let (opening, _) = waits.observe(Some(State::Opening)).expect("set");
    // The view started: the wait for the open is cancelled.
    assert_eq!(waits.observe(Some(State::Open)), None);
    assert!(
        !waits.expired(opening),
        "the open was over; its time is nothing"
    );

    // A later open has its own number; the old one still means nothing.
    let (again, _) = waits.observe(Some(State::Opening)).expect("set");
    assert!(!waits.expired(opening));
    assert!(waits.expired(again), "the one that is set ends the wait");
    assert!(!waits.expired(again), "and only once");
}

#[test]
fn a_state_that_still_waits_after_its_time_sets_the_wait_again() {
    let mut waits = Waits::default();
    let (first, _) = waits.observe(Some(State::Opening)).expect("set");
    assert!(waits.expired(first));
    // The conversation did not leave `Opening` (the news that follows says so):
    // the next look sets another, with a number of its own.
    let (second, delay) = waits.observe(Some(State::Opening)).expect("set again");
    assert_ne!(first, second);
    assert_eq!(delay, OPENING);
    assert!(!waits.expired(first));
    assert!(waits.expired(second));
}

#[test]
fn the_deadlines_are_policy_values_in_the_order_their_reasons_give() {
    assert!(
        OPENING >= Duration::from_secs(10),
        "files cross a link, and the person is asked"
    );
    assert!(
        OFFERED >= Duration::from_secs(10),
        "a program opens again at once, or is busy"
    );
}

#[test]
fn a_token_nobody_took_up_is_forgotten_after_a_time() {
    // After a restart the program is offered a new token, and the conversation is `Offered`:
    // it waits, and the token is gone when the time is up. Before, no deadline ran and
    // a program that never opened again kept an unasked token valid for as long as the pane.
    let mut waits = Waits::default();
    let (offered, delay) = waits.observe(Some(State::Offered)).expect("an offer waits");
    assert_eq!(delay, OFFERED);
    assert_eq!(waits.observe(Some(State::Offered)), None, "once");
    assert!(waits.expired(offered));
}

#[test]
fn an_offer_that_ran_out_is_not_waited_for_again_until_the_conversation_has_been_elsewhere() {
    let mut waits = Waits::default();
    let (first, _) = waits.observe(Some(State::Offered)).expect("set");
    assert!(waits.expired(first));
    // A token for a `hello` is the same state, and the end of its wait changed nothing: a wait
    // set again at every look would run for as long as the pane sits there.
    assert_eq!(waits.observe(Some(State::Offered)), None);
    assert_eq!(waits.observe(Some(State::Offered)), None);
    // The program opened (or was forgotten): another state, and the next offer waits again.
    assert_eq!(
        waits.observe(Some(State::Opening)).map(|(_, d)| d),
        Some(OPENING)
    );
    let (second, _) = waits
        .observe(Some(State::Offered))
        .expect("a new offer waits");
    assert_ne!(first, second);
    assert!(waits.expired(second));
    // A restart offer that follows one that ran out is a new offer even where nothing was
    // observed between.
    assert_eq!(waits.observe(Some(State::Offered)), None);
    waits.restart_offered();
    assert!(waits.observe(Some(State::Offered)).is_some());
}

#[test]
fn an_offer_made_again_while_the_first_is_waited_for_has_a_time_of_its_own() {
    let mut waits = Waits::default();
    let (first, _) = waits.observe(Some(State::Offered)).expect("set");
    // The first offer went nowhere and the program is offered again: a new token, a new time.
    waits.restart_offered();
    let (second, delay) = waits
        .observe(Some(State::Offered))
        .expect("the new offer waits");
    assert_eq!(delay, OFFERED);
    assert_ne!(first, second);
    assert!(
        !waits.expired(first),
        "the first offer's time is not the new one's"
    );
    assert!(waits.expired(second));
}

// ---- the strip and the view's area ------------------------------------------

fn pane(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

#[test]
fn the_strip_and_the_views_area_are_the_pane_exactly() {
    // The strip is the pane's top (the pane is unflipped: the top is the larger y) and the
    // view's area is what is under it, whatever the pane's origin.
    for bounds in [pane(0.0, 0.0, 800.0, 600.0), pane(10.0, 20.0, 333.5, 250.0)] {
        let (strip, area) = (strip_area(bounds), view_area(bounds));
        assert_eq!(strip.size.height, view_strip::HEIGHT);
        assert_eq!(strip.size.width, bounds.size.width);
        assert_eq!(area.size.width, bounds.size.width);
        assert_eq!(area.origin, bounds.origin);
        assert_eq!(area.origin.y + area.size.height, strip.origin.y);
        assert_eq!(
            strip.origin.y + strip.size.height,
            bounds.origin.y + bounds.size.height
        );
        assert_eq!(area.size.height + strip.size.height, bounds.size.height);
    }
}

#[test]
fn a_pane_shorter_than_the_strip_is_all_strip_and_no_view() {
    let bounds = pane(0.0, 0.0, 300.0, view_strip::HEIGHT - 10.0);
    assert_eq!(view_area(bounds).size.height, 0.0);
    assert_eq!(strip_area(bounds).size.height, bounds.size.height);
    assert_eq!(strip_area(bounds).origin.y, 0.0);
    // And nothing is negative for a pane with no size.
    let none = pane(0.0, 0.0, 0.0, 0.0);
    assert_eq!(view_area(none).size.height, 0.0);
    assert_eq!(strip_area(none).size.height, 0.0);
}

#[test]
fn the_keys_modifiers_are_the_four_a_shortcut_uses() {
    let held = modifiers_of(NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
    assert!(held.command && held.shift && !held.control && !held.option);
    // Caps Lock and the function key change nothing about which shortcut a key is.
    let held = modifiers_of(
        NSEventModifierFlags::Command
            | NSEventModifierFlags::CapsLock
            | NSEventModifierFlags::Function,
    );
    assert!(held.command && !held.shift && !held.control && !held.option);
    let held = modifiers_of(NSEventModifierFlags::Control | NSEventModifierFlags::Option);
    assert!(held.control && held.option && !held.command);
}

// ---- the card's picture of a view ---------------------------------------------

/// A page of 800×450 points, a tile of the same shape: all of the page, at the tile's
/// pixels (a tile of 400 pixels at two to the point is 200 points wide).
#[test]
fn a_tile_of_the_pages_shape_takes_the_whole_page() {
    assert_eq!(
        still::capture((800.0, 450.0), 2.0, (400, 225)),
        Some(Capture {
            rect: [0.0, 0.0, 800.0, 450.0],
            width: 200.0
        })
    );
}

/// The terminal's rule, and not another: the picture fills the tile and what spills over is cut
/// off the right and the top, so the left edge and the bottom stay (`bt_gpu::tile_crop`).
#[test]
fn a_tile_of_another_shape_keeps_the_left_edge_and_the_bottom() {
    // A wide page, a tall tile: the right is cut.
    let wide = still::capture((800.0, 400.0), 2.0, (100, 200)).expect("a capture");
    assert_eq!(wide.rect, [0.0, 0.0, 200.0, 400.0]);
    assert_eq!(wide.width, 50.0);
    // A tall page, a wide tile: the top is cut, and the rectangle stands on the bottom.
    let tall = still::capture((400.0, 800.0), 2.0, (200, 100)).expect("a capture");
    assert_eq!(tall.rect, [0.0, 600.0, 400.0, 200.0]);
    assert_eq!(
        tall.rect[1] + tall.rect[3],
        800.0,
        "down to the page's bottom"
    );
    assert_eq!(tall.width, 100.0);
    // Whatever the shapes, the rectangle is the crop of the one rule over the page's pixels.
    for (view, scale, tile) in [
        ((640.0_f64, 368.0_f64), 2.0_f64, (300_u32, 120_u32)),
        ((1000.0, 700.0), 1.0, (120, 300)),
        ((333.0, 777.0), 2.0, (101, 99)),
    ] {
        let [u0, v0, u1, v1] = bt_gpu::tile_crop(
            (
                (view.0 * scale).round() as u32,
                (view.1 * scale).round() as u32,
            ),
            tile,
        );
        let got = still::capture(view, scale, tile).expect("a capture");
        let want = [
            f64::from(u0) * view.0,
            f64::from(v0) * view.1,
            f64::from(u1 - u0) * view.0,
            f64::from(v1 - v0) * view.1,
        ];
        for (got, want) in got.rect.iter().zip(want) {
            assert!(
                (got - want).abs() < 1e-3,
                "{view:?} {tile:?}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn a_page_or_a_tile_with_nothing_in_it_has_no_picture() {
    assert_eq!(still::capture((0.0, 450.0), 2.0, (400, 225)), None);
    assert_eq!(still::capture((800.0, 0.0), 2.0, (400, 225)), None);
    assert_eq!(still::capture((800.0, 450.0), 2.0, (0, 225)), None);
    assert_eq!(still::capture((800.0, 450.0), 2.0, (400, 0)), None);
    assert_eq!(still::capture((800.0, 450.0), 0.0, (400, 225)), None);
}

fn picture() -> objc2_core_foundation::CFRetained<objc2_core_graphics::CGImage> {
    picture_of(2, 2)
}

fn picture_of(
    width: u32,
    height: u32,
) -> objc2_core_foundation::CFRetained<objc2_core_graphics::CGImage> {
    crate::snapshot::image(&bt_gpu::Snapshot {
        width,
        height,
        rgba: vec![255; (width * height * 4) as usize],
    })
    .expect("a picture")
}

/// The view's first picture, taken when it loaded, is what a card finds the first time it
/// asks: the tile is not empty while the card's own request is on its way. Once a picture cut
/// to the tile has come, a first one that is late does not take its place, and one that was
/// taken before the card let go of its pictures is not kept.
#[test]
fn the_first_picture_is_there_for_the_first_card_and_gives_way_to_the_cards_own() {
    use objc2_core_graphics::CGImage;
    use std::time::Instant;
    let now = Instant::now();
    let shared = Shared::default();
    // The view loaded and its picture came before any card asked (generation 0 is a view's
    // own first).
    Still::first_came(&shared, 0, Some(picture_of(8, 4)));
    let (image, ask) = Still::call(&shared, now);
    let image = image.expect("the first call has the first picture");
    assert_eq!(CGImage::width(Some(&image)), 8);
    let generation = ask.expect("and still asks for the tile's own");
    // The tile's own comes and replaces it.
    Still::answered(&shared, generation, Some(picture_of(4, 4)));
    let (image, _) = Still::call(&shared, now + std::time::Duration::from_millis(500));
    assert_eq!(CGImage::width(Some(&image.expect("a picture"))), 4);
    // A first picture that is late does not take the tile's place.
    Still::first_came(&shared, generation, Some(picture_of(8, 4)));
    let (image, _) = Still::call(&shared, now + std::time::Duration::from_millis(600));
    assert_eq!(CGImage::width(Some(&image.expect("a picture"))), 4);
    // The card lets go: what was taken before is not kept after, and a view that gave nothing
    // leaves the card with nothing (the pane shows its terminal then).
    Still::release(&shared);
    Still::first_came(&shared, generation, Some(picture_of(8, 4)));
    assert!(Still::call(&shared, now).0.is_none());
    Still::first_came(&shared, generation + 1, None);
    assert!(Still::call(&shared, now).0.is_none());
}

/// The card asks every half second: a call asks for the next picture and gives the last one
/// that came, a request still on its way is not asked twice, and one that never comes back is
/// asked again.
#[test]
fn the_card_is_given_the_last_picture_and_one_request_is_in_flight() {
    use std::time::Instant;
    let shared = Shared::default();
    let start = Instant::now();
    let (image, ask) = Still::call(&shared, start);
    assert!(image.is_none(), "nothing has come yet");
    let generation = ask.expect("the first call asks");
    // The next turns of the card find it on its way.
    for later in [100, 500, 1900] {
        let (image, ask) = Still::call(&shared, start + std::time::Duration::from_millis(later));
        assert!(image.is_none() && ask.is_none(), "after {later} ms");
    }
    // It comes: the next call has it, and asks for the next.
    Still::answered(&shared, generation, Some(picture()));
    let (image, ask) = Still::call(&shared, start + std::time::Duration::from_millis(500));
    assert!(image.is_some(), "the picture that came");
    let generation = ask.expect("and asks again");
    // An answer with nothing (a view that could not be shot) leaves the last picture.
    Still::answered(&shared, generation, None);
    let (image, ask) = Still::call(&shared, start + std::time::Duration::from_millis(1000));
    assert!(image.is_some() && ask.is_some());
    // A request that does not come back is asked again after the patience, not before.
    let late = start + std::time::Duration::from_millis(1000);
    let (_, ask) = Still::call(&shared, late + PATIENCE / 2);
    assert!(ask.is_none());
    let (_, ask) = Still::call(&shared, late + PATIENCE);
    assert!(ask.is_some());
}

/// The card letting go of its pictures lets go of the last one, and an answer to a request
/// made before is not kept: the picture of a card that is gone would otherwise stay as long
/// as the view.
#[test]
fn a_picture_that_comes_after_the_card_let_go_is_not_kept() {
    use std::time::Instant;
    let shared = Shared::default();
    let now = Instant::now();
    let (_, ask) = Still::call(&shared, now);
    let before = ask.expect("a request");
    Still::answered(&shared, before, Some(picture()));
    assert!(Still::call(&shared, now).0.is_some());
    Still::release(&shared);
    assert!(Still::call(&shared, now).0.is_none(), "the last one went");
    // The old request comes back after the release.
    Still::answered(&shared, before, Some(picture()));
    assert!(
        Still::call(&shared, now).0.is_none(),
        "and the late one is not kept"
    );
}

// ---- the files that are read off the main thread ------------------------------

#[test]
fn only_the_files_of_a_bundle_are_read_off_the_main_thread() {
    let page = bundle_page();
    assert_eq!(
        page.file_of(&url("index.html")).as_deref(),
        Some("index.html")
    );
    assert_eq!(
        page.file_of(&url("app/main.js")).as_deref(),
        Some("app/main.js")
    );
    // The shell page, a path the page refuses and another view's origin are answered from
    // memory: nothing waits for a file.
    assert_eq!(page.file_of(&Page::shell_url()), None);
    assert_eq!(page.file_of(&url("../etc/passwd")), None);
    assert_eq!(
        page.file_of("bateri-view://v-ffffffffffffffffffffffffffffffff/index.html"),
        None
    );
    // A reply that is not a file's never asks the file service.
    let asked = std::cell::Cell::new(false);
    for text in [Page::shell_url(), url("../x")] {
        page.reply(&text, &|_| {
            asked.set(true);
            None
        });
    }
    assert!(!asked.get());
}

#[test]
fn a_task_is_answered_when_its_file_is_ready_and_only_if_it_is_still_held() {
    let mut pending = Pending::new();
    let (view, other) = (1usize, 2usize);
    let kept = pending.hold(view, "kept");
    let stopped = pending.hold(view, "stopped");
    let theirs = pending.hold(other, "theirs");
    // The web view stopped one before its file came back.
    assert!(pending.stop(|task| *task == "stopped"));
    assert!(!pending.stop(|task| *task == "stopped"), "once");
    assert_eq!(
        pending.finish(stopped),
        None,
        "a stopped task is not answered"
    );
    assert_eq!(pending.finish(kept), Some("kept"));
    assert_eq!(pending.finish(kept), None, "and answered once");
    // A view that closes takes its tasks with it and no one else's.
    let late = pending.hold(view, "late");
    pending.forget(view);
    assert_eq!(pending.finish(late), None);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending.finish(theirs), Some("theirs"));
}

#[test]
fn the_scheme_handler_reads_a_file_off_the_main_thread_and_answers_on_it() {
    // The call of the file service (`Session::serve_view_file`: it waits for the conversation
    // and reads and checks up to 8 MiB) is made on the file queue, and the task is
    // answered back on the main queue from the reply that came. A call in the handler's own
    // body would be the main thread waiting.
    let source = include_str!("../pencere_view.rs");
    let at = |text: &str| source.find(text).unwrap_or_else(|| panic!("`{text}`"));
    let answer = &source[at("    fn answer(")..at("    fn respond(")];
    assert!(
        !answer.contains("serve_view_file"),
        "the handler's body does not call the file service"
    );
    assert!(
        answer.matches(".reply(").count() == 1 && answer.contains("&|_| None"),
        "the one reply the handler's body makes is the one that asks for no file"
    );
    assert!(answer.contains("file_of("));
    let queued = &answer[answer.find("files_queue()").expect("the file queue")..];
    let (on_queue, back) = queued
        .split_once("DispatchQueue::main()")
        .expect("and back on the main queue");
    assert!(
        on_queue.contains("read_file_reply("),
        "the file is read on the queue"
    );
    assert!(
        back.contains("respond("),
        "and the task is answered on the main queue"
    );
    // The one call is the queue's reader.
    let reader = &source[at("fn read_file_reply(")..];
    assert!(reader.contains("serve_view_file"));
    let calls = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//") && line.contains("serve_view_file"))
        .count();
    assert_eq!(calls, 1, "the file service is called from one place");
}

#[test]
fn the_view_asks_the_systems_random_bytes_of_the_core_and_not_of_libc() {
    let source = include_str!("../pencere_view.rs");
    assert!(!source.contains("getentropy"));
    assert!(source.contains("system_random()"));
}
