//! What the application serves to a view's web view, and what it lets that web
//! view load — the part of the view that has no AppKit in it.
//!
//! A view runs on two documents from two origins, both served by the
//! application under a URL scheme of its own (there is no server, no port and no
//! file):
//!
//! * **the shell**, `bateri-view://shell/`: a page of ours with the one
//!   `<iframe>` the view lives in, and the script that carries the JSON-RPC
//!   messages between that frame and the application (`window.postMessage` ↔
//!   the script-message handler). Its policy lets it frame the view's origin and
//!   nothing else;
//! * **the view**, `bateri-view://<instance>/…`: the entry document and the
//!   other files of the bundle — or the one document the program's
//!   `resources/read` gave — each with the policy the crate built from what the
//!   person allowed. `<instance>` is random and belongs to this one start of one
//!   view (§8.6): never the shell's, never another view's.
//!
//! A view therefore sits one origin away from the page that holds the bridge,
//! which is what keeps it from reaching it: it has `window.parent` and
//! `postMessage`, nothing of the shell's script or of the application. (Why not
//! the view as the top document: MCP Apps views write to `window.parent`, so an
//! unmodified view would have to be adapted by a script of ours inside its own
//! page.)
//!
//! Everything here is a pure function of strings and bytes, so what a request
//! may reach, which headers it gets and what the shell page says are tested
//! without a web view.

use std::sync::Arc;

/// The URL scheme of the two documents, served by this application's
/// `WKURLSchemeHandler` for the web view of one view and for no other.
pub(super) const SCHEME: &str = "bateri-view";

/// The shell's host name: fixed, since it is the application's own page and each
/// web view has its own handler that serves one view's instance only.
const SHELL_HOST: &str = "shell";

/// The path the document of a view that came as HTML (with no bundle) is served
/// at. It has no other file: everything else it asks for is a 404.
pub(super) const DOCUMENT_PATH: &str = "view.html";

/// The `sandbox` attribute of the view's frame. MCP Apps' own: scripts, and the
/// view's own origin (so it has storage of its own, which never outlives the
/// web view — the data store is not persistent) — and nothing that leaves the
/// frame: no navigation of the top page, no popups, no modals, no downloads.
pub(super) const SANDBOX: &str = "allow-scripts allow-same-origin allow-forms";

/// What a URL the web view asks for names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Route {
    /// The shell page.
    Shell,
    /// A file of the view, by the path the manifest names it with (or the
    /// document's [`DOCUMENT_PATH`]).
    View(String),
    /// Anything else: another scheme, another host, another instance's origin,
    /// a path that is not one.
    Refused,
}

/// The `scheme://` of a URL, if it is [`SCHEME`] (the scheme is case-blind).
fn after_scheme(url: &str) -> Option<&str> {
    let head = url.get(..SCHEME.len())?;
    if !head.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    url[SCHEME.len()..].strip_prefix("://")
}

/// A URL's host and path: what is between `://` and the first `?` or `#`, cut at
/// the first `/`. A port or a user name stays in the host, which then names no
/// origin of ours.
fn host_and_path(url: &str) -> Option<(&str, &str)> {
    let rest = after_scheme(url)?;
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = &rest[..end];
    Some(match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    })
}

/// `%XX` escapes undone; `None` for a broken escape or bytes that are not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let pair = bytes.get(at + 1..at + 3)?;
            let digit = |byte: u8| char::from(byte).to_digit(16);
            let high = digit(pair[0])?;
            let low = digit(pair[1])?;
            out.push(u8::try_from(high * 16 + low).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A path escaped for a URL: what is not an unreserved character or a `/` is
/// `%XX`, so a file name with a space or a `?` in it is still one path.
fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// What `url` names, for the view of `instance`.
///
/// A path of the view is taken **decoded and bare**: no `.` or `..` segment, no
/// empty segment (`//`), no NUL or backslash, nothing empty. The manifest's own
/// paths are like that (§8.2); this is what keeps a name that only looks like one
/// from being asked of the crate at all, and the crate answers for the manifest's
/// files and for nothing else.
pub(super) fn route(instance: &str, url: &str) -> Route {
    let Some((host, path)) = host_and_path(url) else {
        return Route::Refused;
    };
    if host.eq_ignore_ascii_case(SHELL_HOST) {
        return if path.is_empty() || path == "/" {
            Route::Shell
        } else {
            Route::Refused
        };
    }
    if !host.eq_ignore_ascii_case(instance) {
        return Route::Refused;
    }
    let Some(path) = path.strip_prefix('/').and_then(percent_decode) else {
        return Route::Refused;
    };
    let bare = !path.is_empty()
        && !path.contains(['\0', '\\'])
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if bare {
        Route::View(path)
    } else {
        Route::Refused
    }
}

/// `#rrggbb`, the way a CSS colour is written from the theme's sRGB bytes.
pub(super) fn css_color([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `text` as an HTML attribute's value.
fn escape_attribute(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

/// `text` as a JavaScript string literal, to be a call's argument: quotes and
/// backslashes escaped, control characters and the two line separators written
/// as escapes, so whatever the text holds is the string and never the code.
pub(super) fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() || c == '\u{2028}' || c == '\u{2029}' => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The shell's call into the page that hands the view one message (`text` is
/// JSON the crate wrote).
pub(super) fn deliver_script(text: &str) -> String {
    format!("window.pencere.deliver({})", js_string(text))
}

/// The shell's call that gives the keyboard to the view's frame.
pub(super) const FOCUS_SCRIPT: &str = "window.pencere.focus()";

/// The shell's call that repaints the page's own ground (`color` is
/// [`css_color`]'s).
pub(super) fn ground_script(color: &str) -> String {
    format!("window.pencere.ground({})", js_string(color))
}

/// The shell page: the frame of the view, and the bridge.
///
/// The frame's `sandbox` is [`SANDBOX`] and its `allow` is the crate's
/// (`ViewStart::allow`, the permissions the person approved). The script takes a
/// message **only from that frame's window** and passes it on as the JSON text
/// it serialises to (a value JSON cannot hold is dropped); it hands the
/// application's messages to the frame with `postMessage`. A message is never
/// built from anything but `JSON.stringify` / `JSON.parse`, so nothing the view
/// sends is ever code here.
fn shell_html(view_url: &str, allow: &str, ground: &str) -> String {
    format!(
        r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
html, body {{ margin: 0; padding: 0; height: 100%; overflow: hidden; background: {ground}; }}
iframe {{ display: block; border: 0; width: 100%; height: 100%; background: transparent; }}
</style>
</head>
<body>
<iframe id="view" src="{src}" sandbox="{sandbox}" allow="{allow}"></iframe>
<script>
(function () {{
  var frame = document.getElementById("view");
  var host = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.pencere;
  function focusView() {{
    try {{ frame.focus(); frame.contentWindow.focus(); }} catch (error) {{}}
  }}
  frame.addEventListener("load", focusView);
  window.addEventListener("focus", focusView);
  window.addEventListener("message", function (event) {{
    if (!host || event.source !== frame.contentWindow) return;
    var text;
    try {{ text = JSON.stringify(event.data); }} catch (error) {{ return; }}
    if (typeof text === "string") host.postMessage(text);
  }});
  window.pencere = {{
    deliver: function (text) {{
      var message;
      try {{ message = JSON.parse(text); }} catch (error) {{ return; }}
      if (frame.contentWindow) frame.contentWindow.postMessage(message, "*");
    }},
    focus: focusView,
    ground: function (color) {{
      document.documentElement.style.background = color;
      document.body.style.background = color;
    }}
  }};
}})();
</script>
</body>
</html>
"#,
        ground = escape_attribute(ground),
        src = escape_attribute(view_url),
        sandbox = SANDBOX,
        allow = escape_attribute(allow),
    )
}

/// Where the view's document comes from.
#[derive(Clone, Debug)]
pub(super) enum Document {
    /// The entry of the bundle: a path of its manifest. The files are the
    /// crate's to give (`Session::serve_view_file`).
    Bundle(String),
    /// The one HTML document the program's `resources/read` returned.
    Html(Arc<[u8]>),
}

/// A file of the bundle as the crate's file service gives it: its media type and its
/// bytes.
pub(super) type Served = (String, Arc<[u8]>);

/// One answer to one request.
#[derive(Clone, Debug)]
pub(super) struct Reply {
    /// The HTTP status.
    pub(super) status: i16,
    /// The headers, name and value.
    pub(super) headers: Vec<(&'static str, String)>,
    /// The body.
    pub(super) body: Arc<[u8]>,
}

impl Reply {
    /// The value of the header `name`, if the reply has it.
    #[cfg(test)]
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// The headers every answer carries, then its own: the type, the policy, the
/// length, and a refusal to be cached, sniffed or to name where it was linked from.
fn headers(media: &str, policy: &str, length: usize) -> Vec<(&'static str, String)> {
    vec![
        ("Content-Type", media.to_owned()),
        ("Content-Security-Policy", policy.to_owned()),
        ("Content-Length", length.to_string()),
        ("Cache-Control", "no-store".to_owned()),
        ("X-Content-Type-Options", "nosniff".to_owned()),
        ("Referrer-Policy", "no-referrer".to_owned()),
    ]
}

/// What a request that names nothing of ours gets.
fn not_found() -> Reply {
    Reply {
        status: 404,
        headers: headers("text/plain; charset=utf-8", "default-src 'none'", 0),
        body: Arc::from(Vec::new()),
    }
}

/// What one view's web view serves and loads: built when the view starts, for
/// that view only.
#[derive(Debug)]
pub(super) struct Page {
    /// The random host of the view's origin.
    instance: String,
    document: Document,
    /// The crate's policy, from what the person allowed: on every file of the
    /// view, as the response header.
    policy: String,
    /// The shell page, built once.
    shell: Arc<[u8]>,
}

impl Page {
    /// The page of the view of `instance` — a name that is lowercase hex of
    /// random bytes, new for every view ([`instance`]).
    ///
    /// `policy` and `allow` are the crate's, as it gave them with the view;
    /// `ground` is the pane's background as a CSS colour.
    pub(super) fn new(
        instance: String,
        document: Document,
        policy: String,
        allow: &str,
        ground: &str,
    ) -> Self {
        let entry = match &document {
            Document::Bundle(entry) => entry.as_str(),
            Document::Html(_) => DOCUMENT_PATH,
        };
        let view_url = format!("{SCHEME}://{instance}/{}", percent_encode_path(entry));
        let shell = shell_html(&view_url, allow, ground).into_bytes().into();
        Self {
            instance,
            document,
            policy,
            shell,
        }
    }

    /// The URL the web view loads: the shell.
    pub(super) fn shell_url() -> String {
        format!("{SCHEME}://{SHELL_HOST}/")
    }

    /// The policy of the shell page: it frames the view's origin and nothing
    /// else, runs its own script and loads nothing.
    fn shell_policy(&self) -> String {
        format!(
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
             frame-src {SCHEME}://{}; base-uri 'none'; form-action 'none'",
            self.instance
        )
    }

    /// The answer to a request for `url`. `files` is the crate's file service
    /// (`Session::serve_view_file`): the bytes and media type of a file of the
    /// bundle, or nothing for a path the manifest does not name.
    pub(super) fn reply(&self, url: &str, files: &dyn Fn(&str) -> Option<Served>) -> Reply {
        match route(&self.instance, url) {
            Route::Shell => Reply {
                status: 200,
                headers: headers(
                    "text/html; charset=utf-8",
                    &self.shell_policy(),
                    self.shell.len(),
                ),
                body: Arc::clone(&self.shell),
            },
            Route::View(path) => {
                let served = match &self.document {
                    Document::Bundle(_) => files(&path),
                    Document::Html(html) if path == DOCUMENT_PATH => {
                        Some(("text/html; charset=utf-8".to_owned(), Arc::clone(html)))
                    }
                    Document::Html(_) => None,
                };
                match served {
                    Some((media, bytes)) => Reply {
                        status: 200,
                        headers: headers(&media, &self.policy, bytes.len()),
                        body: bytes,
                    },
                    None => not_found(),
                }
            }
            Route::Refused => not_found(),
        }
    }

    /// Whether the web view may navigate to `url`: the shell, once, as the top
    /// document; in a frame, the view's own origin (and a blank frame, which has
    /// no origin to leave). Nothing else, whoever asks: a link, a script, a form,
    /// a redirect.
    pub(super) fn allows(&self, url: &str, main_frame: bool, shell_loaded: bool) -> bool {
        if main_frame {
            return !shell_loaded && route(&self.instance, url) == Route::Shell;
        }
        matches!(route(&self.instance, url), Route::View(_))
            || url.eq_ignore_ascii_case("about:blank")
    }
}

/// The host of a view's origin, from 16 random bytes: lowercase hex, prefixed so
/// it can never be the shell's name.
pub(super) fn instance(bytes: [u8; 16]) -> String {
    let mut name = String::from("v-");
    for byte in bytes {
        name.push_str(&format!("{byte:02x}"));
    }
    name
}
