//! The pure parts of a program's view: what its web view may reach, the headers
//! and the shell page it is given, and the deadlines.

use std::sync::Arc;
use std::time::Duration;

use bt_core::pencere::State;
use objc2_app_kit::NSEventModifierFlags;

use super::is_interrupt;
use super::serve::{
    self, DOCUMENT_PATH, Document, Page, Route, SANDBOX, Served, css_color, deliver_script,
    ground_script, js_string, route,
};
use super::wait::{OPENING, TEARDOWN, Waits};

const INSTANCE: &str = "v-0123456789abcdef0123456789abcdef";
const POLICY: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; connect-src 'self' https://api.example;";

fn url(path: &str) -> String {
    format!("bateri-view://{INSTANCE}/{path}")
}

fn bundle_page() -> Page {
    Page::new(
        INSTANCE.to_owned(),
        Document::Bundle("index.html".to_owned()),
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
fn a_view_that_came_as_html_has_one_document_and_nothing_else() {
    let html: Arc<[u8]> = Arc::from(&b"<p>hello</p>"[..]);
    let page = Page::new(
        INSTANCE.to_owned(),
        Document::Html(Arc::clone(&html)),
        POLICY.to_owned(),
        "",
        "#000000",
    );
    let reply = page.reply(&url(DOCUMENT_PATH), &files);
    assert_eq!(reply.status, 200);
    assert_eq!(&*reply.body, &*html);
    assert_eq!(
        reply.header("Content-Type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(reply.header("Content-Security-Policy"), Some(POLICY));
    // The bundle's files are not this view's, even when the service has them.
    for other in ["index.html", "app.js", "other.html"] {
        assert_eq!(page.reply(&url(other), &files).status, 404, "{other}");
    }
    // Its frame is at the document's path.
    let shell = String::from_utf8(page.reply(&Page::shell_url(), &files).body.to_vec())
        .expect("the shell is text");
    assert!(
        shell.contains(&format!("src=\"{}\"", url(DOCUMENT_PATH))),
        "{shell}"
    );
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
        Document::Bundle("a\"><script>x()</script>.html".to_owned()),
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
        text.contains("window.webkit.messageHandlers.pencere"),
        "the one handler the application registers: {text}"
    );
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
    assert!(page.allows(&shell, true, false));
    assert!(
        !page.allows(&shell, true, true),
        "a reload is the shell loaded again"
    );
    assert!(
        !page.allows(&url("index.html"), true, false),
        "the view is not a top document"
    );
    assert!(!page.allows("https://example.com/", true, false));
    assert!(!page.allows("file:///etc/passwd", true, false));
    assert!(!page.allows("about:blank", true, false));
}

#[test]
fn a_frame_may_load_the_views_origin_and_a_blank_page_and_nothing_else() {
    let page = bundle_page();
    assert!(page.allows(&url("index.html"), false, true));
    assert!(page.allows(&url("dir/other.html"), false, true));
    assert!(page.allows("about:blank", false, true));
    assert!(
        !page.allows(&Page::shell_url(), false, true),
        "the shell inside the shell"
    );
    assert!(!page.allows("https://example.com/", false, true));
    assert!(!page.allows("http://localhost:8080/", false, true));
    assert!(!page.allows(
        "bateri-view://v-ffffffffffffffffffffffffffffffff/index.html",
        false,
        true
    ));
    assert!(!page.allows(&url("../index.html"), false, true));
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
fn control_c_alone_is_the_interrupt() {
    let control = NSEventModifierFlags::Control;
    assert!(is_interrupt(control, Some("\u{3}")));
    // Caps Lock and the numeric pad's flags are not modifiers that change it.
    assert!(is_interrupt(
        control | NSEventModifierFlags::CapsLock,
        Some("\u{3}")
    ));
    for flags in [
        NSEventModifierFlags::empty(),
        control | NSEventModifierFlags::Shift,
        control | NSEventModifierFlags::Command,
        control | NSEventModifierFlags::Option,
        NSEventModifierFlags::Command,
    ] {
        assert!(!is_interrupt(flags, Some("\u{3}")), "{flags:?}");
    }
    assert!(!is_interrupt(control, Some("c")), "a letter is the view's");
    assert!(!is_interrupt(control, Some("\u{4}")), "⌃D is not ⌃C");
    assert!(!is_interrupt(control, None));
    assert!(!is_interrupt(NSEventModifierFlags::empty(), Some("c")));
}

// ---- the deadlines ----------------------------------------------------------

#[test]
fn a_wait_is_set_once_for_the_state_that_waits_and_for_no_other() {
    let mut waits = Waits::default();
    for quiet in [State::Idle, State::Offered, State::Open, State::Ended] {
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
    let (closing, delay) = waits
        .observe(Some(State::Closing))
        .expect("a teardown waits");
    assert_eq!(delay, TEARDOWN);
    assert_ne!(opening, closing);
    assert_eq!(waits.observe(Some(State::Closing)), None);
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

    // A later teardown has its own number; the old one still means nothing.
    let (closing, _) = waits.observe(Some(State::Closing)).expect("set");
    assert!(!waits.expired(opening));
    assert!(waits.expired(closing), "the one that is set ends the wait");
    assert!(!waits.expired(closing), "and only once");
}

#[test]
fn a_state_that_still_waits_after_its_time_sets_the_wait_again() {
    let mut waits = Waits::default();
    let (first, _) = waits.observe(Some(State::Closing)).expect("set");
    assert!(waits.expired(first));
    // The conversation did not leave `Closing` (the news that follows says so):
    // the next look sets another, with a number of its own.
    let (second, delay) = waits.observe(Some(State::Closing)).expect("set again");
    assert_ne!(first, second);
    assert_eq!(delay, TEARDOWN);
    assert!(!waits.expired(first));
    assert!(waits.expired(second));
}

#[test]
fn the_deadlines_are_policy_values_in_the_order_their_reasons_give() {
    assert!(
        OPENING > TEARDOWN,
        "files cross a link; a view that answers does not"
    );
    assert!(TEARDOWN >= Duration::from_secs(1));
}
