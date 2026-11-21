//! The loopback proxy every request of the helper goes through (Karar 2).
//!
//! It is open before the helper starts and its port is on the helper's command line for the
//! helper's whole life. It listens on `127.0.0.1` only and wants credentials: every profile the
//! helper opens has its own (a user and a random password, [`Proxy::allow`]), and a connection
//! that has none, or wrong ones, gets `407` and nothing else. That is also what stops Chromium's
//! own background requests (updates, the clock, components): they carry no identity, so they go
//! nowhere. A process of this Mac that is not the helper cannot use it either: it does not know a
//! password.
//!
//! The request of a profile that is known goes by the way its machine has been shown to have
//! ([`super::route`]): a `CONNECT` is a tunnel, and a request with an absolute address is made to
//! that address with its own head (origin form, no `Proxy-Authorization`: the identity of the
//! browser must not reach a dev server), the connection to each address kept for the next request of
//! the same client connection. A machine with no way here gets `502` and the reason.
//!
//! **The proxy's own `502`s say what failed to the helper, not only to the person**: the body is
//! the sentence, and the header [`NET_ERROR`] is the Chromium net error it stands for (`-102`
//! refused, `-118` timed out, `-105` name not resolved, `-130` no way to the program's machine,
//! `-109` any other), which the helper turns into the page's load error. The header is the proxy's
//! alone: a server's answer that has it loses it on the way back ([`answers`]).

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::profile::ProfileKey;
use super::route::{DIAL_WAIT, Dialer, Link, Parts, Routes};

mod answers;

/// The header of the proxy's `502` that names the Chromium net error the failure is (the helper
/// reads it on a `502` of a page's own document).
pub const NET_ERROR: &str = "X-Bateri-Net-Error";
/// `ERR_CONNECTION_REFUSED`.
const REFUSED: i32 = -102;
/// `ERR_NAME_NOT_RESOLVED`.
const NAME_NOT_RESOLVED: i32 = -105;
/// `ERR_ADDRESS_UNREACHABLE`: any failure that is none of the others.
const UNREACHABLE: i32 = -109;
/// `ERR_CONNECTION_TIMED_OUT`.
const TIMED_OUT: i32 = -118;
/// `ERR_PROXY_CONNECTION_FAILED`: there is no way to the program's machine.
const NO_WAY: i32 = -130;

/// The largest request head the proxy reads: a bigger one is not a browser's.
const HEAD_MAX: usize = 16 * 1024;
/// How long a connection may take to say what it wants.
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);

/// What a proxy user is allowed: the password and the profile the user stands for.
#[derive(Clone, Debug)]
struct Identity {
    password: String,
    profile: ProfileKey,
}

struct Registry {
    identities: Mutex<HashMap<String, Identity>>,
    /// The way each machine's connections are made by, once it has been shown.
    routes: Arc<Routes>,
}

impl Registry {
    /// The profile a `Proxy-Authorization` header names, if its user and password are known.
    fn profile_of(&self, authorization: &str) -> Option<ProfileKey> {
        let encoded = authorization.trim().strip_prefix("Basic ")?;
        let decoded = String::from_utf8(base64_decode(encoded.trim())?).ok()?;
        let (user, password) = decoded.split_once(':')?;
        let identities = self.identities.lock().ok()?;
        let identity = identities.get(user)?;
        same(identity.password.as_bytes(), password.as_bytes()).then(|| identity.profile.clone())
    }
}

/// An open proxy: stops listening when dropped.
pub struct Proxy {
    port: u16,
    registry: Arc<Registry>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Proxy {
    /// Starts listening on a port of the loopback the system chooses. What a profile's
    /// connections go by is looked up in `routes`, by its machine, as they are made.
    pub fn bind(routes: Arc<Routes>) -> io::Result<Proxy> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let registry = Arc::new(Registry {
            identities: Mutex::new(HashMap::new()),
            routes,
        });
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (registry, stop) = (registry.clone(), stop.clone());
            std::thread::Builder::new()
                .name("browser-proxy".into())
                .spawn(move || accept(&listener, &registry, &stop))?
        };
        Ok(Proxy {
            port,
            registry,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// What goes on the helper's command line (`--proxy=`).
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Lets `user` with `password` through as `profile`.
    pub fn allow(&self, user: &str, password: &str, profile: &ProfileKey) {
        if let Ok(mut identities) = self.registry.identities.lock() {
            identities.insert(
                user.to_owned(),
                Identity {
                    password: password.to_owned(),
                    profile: profile.clone(),
                },
            );
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn accept(listener: &TcpListener, registry: &Arc<Registry>, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let registry = registry.clone();
                let _ = std::thread::Builder::new()
                    .name("browser-proxy-connection".into())
                    .spawn(move || {
                        let _ = serve(stream, &registry);
                    });
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn serve(mut stream: TcpStream, registry: &Registry) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(HEAD_TIMEOUT))?;
    stream.set_write_timeout(Some(HEAD_TIMEOUT))?;
    let head = read_head(&mut stream)?;
    let known = header(&head, "proxy-authorization").and_then(|value| registry.profile_of(&value));
    let Some(profile) = known else {
        return end_with(&mut stream, &unauthorized());
    };
    let Some(way) = registry.routes.get(profile.machine()) else {
        return end_with(
            &mut stream,
            &bad_gateway(
                NO_WAY,
                "There is no route from this pane to the program's machine.",
            ),
        );
    };
    Carrying {
        client: stream,
        registry,
        profile,
        way,
        upstreams: HashMap::new(),
        last: Arc::new(Mutex::new(None)),
    }
    .run(head)
}

fn unauthorized() -> String {
    reply(
        407,
        "Proxy Authentication Required",
        "Proxy-Authenticate: Basic realm=\"bateri\"\r\n",
        "",
    )
}

fn end_with(stream: &mut TcpStream, answer: &str) -> io::Result<()> {
    stream.write_all(answer.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

/// A connection to an address that a client connection's requests were made to, kept for the next.
struct Upstream {
    writer: Box<dyn Write + Send>,
    end: Arc<dyn Fn() + Send + Sync>,
    /// Set by the thread that carries its answers back when the far end has ended.
    dead: Arc<AtomicBool>,
    /// The requests sent on it that have no final answer yet (the answers are read as they pass).
    asking: Arc<answers::Asking>,
}

/// One client connection of a known profile, and the connections made for it.
struct Carrying<'a> {
    client: TcpStream,
    registry: &'a Registry,
    profile: ProfileKey,
    way: Arc<dyn Dialer>,
    /// By address (`host:port`).
    upstreams: HashMap<String, Upstream>,
    /// The address the last request went to: when its connection ends, the answer it was
    /// carrying ended with it, and the client is told so.
    last: Arc<Mutex<Option<String>>>,
}

impl Carrying<'_> {
    /// Serves requests until the client or the far end is done. `first` is the first head, already
    /// read.
    fn run(mut self, first: String) -> io::Result<()> {
        let result = self.requests(first);
        for upstream in self.upstreams.values() {
            (upstream.end)();
        }
        let _ = self.client.shutdown(Shutdown::Both);
        result
    }

    fn requests(&mut self, first: String) -> io::Result<()> {
        let mut head = first;
        loop {
            if !self.same_identity(&head) {
                return self.say(&unauthorized());
            }
            let line = head.lines().next().unwrap_or_default().to_owned();
            let mut words = line.split(' ');
            let (Some(method), Some(target), Some(version)) =
                (words.next(), words.next(), words.next())
            else {
                return self.say(&reply(400, "Bad Request", "", "Not a request.\n"));
            };
            if method.eq_ignore_ascii_case("CONNECT") {
                return self.tunnel(target);
            }
            let Some((authority, path)) = split_target(target) else {
                return self.say(&reply(
                    400,
                    "Bad Request",
                    "",
                    "A request is made to an address with http:// or is a CONNECT.\n",
                ));
            };
            let upgrade = header(&head, "upgrade").is_some();
            self.forward(&head, method, &authority, &path, version)?;
            if upgrade {
                return self.raw(&authority);
            }
            // The client has the answer before it says more: no timeout while it waits for one.
            self.client.set_read_timeout(None)?;
            head = match read_head(&mut self.client) {
                Ok(head) => head,
                // The client is done with this connection: ended, or silent for good.
                Err(_) => return Ok(()),
            };
        }
    }

    /// Whether a request of this client connection names the identity the connection began with.
    fn same_identity(&self, head: &str) -> bool {
        header(head, "proxy-authorization")
            .and_then(|value| self.registry.profile_of(&value))
            .is_some_and(|profile| profile == self.profile)
    }

    fn say(&mut self, answer: &str) -> io::Result<()> {
        self.client.write_all(answer.as_bytes())?;
        self.client.flush()
    }

    /// Makes the connection to `authority` by the way, and starts carrying what it says back.
    fn dial(&mut self, authority: &str) -> Result<(), String> {
        let (host, port) = split_authority(authority, 80)
            .ok_or_else(|| format!("{authority} is not an address"))?;
        let link = self.way.dial(&host, port, DIAL_WAIT)?;
        let Parts {
            reader,
            writer,
            end,
            hold,
        } = link.split();
        let dead = Arc::new(AtomicBool::new(false));
        let client = self.client.try_clone().map_err(|e| e.to_string())?;
        let (last, key) = (Arc::clone(&self.last), authority.to_owned());
        let (flag, closing) = (Arc::clone(&dead), Arc::clone(&end));
        let asking = Arc::new(Mutex::new(VecDeque::new()));
        let asked = Arc::clone(&asking);
        std::thread::Builder::new()
            .name("browser-proxy-answers".into())
            .spawn(move || {
                let _hold = hold;
                let mut client = client;
                let _ = answers::carry(reader, &mut client, &asked);
                flag.store(true, Ordering::Release);
                closing();
                // What the last request asked is answered to its end by this connection's end: the
                // client waits for no more.
                if lock(&last).as_deref() == Some(key.as_str()) {
                    let _ = client.shutdown(Shutdown::Write);
                }
            })
            .map_err(|e| e.to_string())?;
        self.upstreams.insert(
            authority.to_owned(),
            Upstream {
                writer,
                end,
                dead,
                asking,
            },
        );
        Ok(())
    }

    /// Sends a request on, with its body, to the connection of its address (made if there is none
    /// or its far end has ended).
    fn forward(
        &mut self,
        head: &str,
        method: &str,
        authority: &str,
        path: &str,
        version: &str,
    ) -> io::Result<()> {
        let reusable = self
            .upstreams
            .get(authority)
            .is_some_and(|up| !up.dead.load(Ordering::Acquire));
        if !reusable {
            if let Some(old) = self.upstreams.remove(authority) {
                (old.end)();
            }
            if let Err(why) = self.dial(authority) {
                self.say(&bad_gateway(net_error(&why), &why))?;
                return Err(io::Error::other(why));
            }
        }
        *lock(&self.last) = Some(authority.to_owned());
        let out = rewritten(head, method, path, version);
        let body = body_of(head);
        let client = &mut self.client;
        let Some(up) = self.upstreams.get_mut(authority) else {
            return Err(io::Error::other("no connection"));
        };
        lock(&up.asking).push_back(method.eq_ignore_ascii_case("HEAD"));
        up.writer.write_all(out.as_bytes())?;
        match body {
            Body::None => {}
            Body::Length(length) => {
                io::copy(&mut client.take(length), &mut up.writer)?;
            }
            Body::Chunked => copy_chunked(client, &mut up.writer)?,
        }
        up.writer.flush()
    }

    /// `CONNECT host:port`: a tunnel until either end is done.
    fn tunnel(&mut self, target: &str) -> io::Result<()> {
        let Some((host, port)) = split_authority(target, 443) else {
            return self.say(&reply(400, "Bad Request", "", "Not an address.\n"));
        };
        let link = match self.way.dial(&host, port, DIAL_WAIT) {
            Ok(link) => link,
            Err(why) => return self.say(&bad_gateway(net_error(&why), &why)),
        };
        self.client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        self.client.flush()?;
        self.client.set_read_timeout(None)?;
        self.pipe(link)
    }

    /// After an `Upgrade` the connection is whatever the two ends make of it.
    fn raw(&mut self, authority: &str) -> io::Result<()> {
        let Some(up) = self.upstreams.remove(authority) else {
            return Ok(());
        };
        self.client.set_read_timeout(None)?;
        // The answers are already carried back by the thread of the connection: only what the
        // client says now goes on.
        let mut writer = up.writer;
        let result = io::copy(&mut self.client, &mut writer).map(|_| ());
        (up.end)();
        result
    }

    /// Copies both ways between the client and `link` until one has no more to say.
    fn pipe(&mut self, link: Link) -> io::Result<()> {
        let Parts {
            mut reader,
            mut writer,
            end,
            hold,
        } = link.split();
        let mut to_client = self.client.try_clone()?;
        let closing = Arc::clone(&end);
        let answers = std::thread::Builder::new()
            .name("browser-proxy-tunnel".into())
            .spawn(move || {
                let _hold = hold;
                let _ = io::copy(&mut reader, &mut to_client);
                let _ = to_client.shutdown(Shutdown::Both);
                closing();
            })?;
        let _ = io::copy(&mut self.client, &mut writer);
        end();
        let _ = answers.join();
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `http://host:port/path?query` as the address and the path of the request that is made to it.
fn split_target(target: &str) -> Option<(String, String)> {
    let scheme = target.get(..7)?;
    if !scheme.eq_ignore_ascii_case("http://") {
        return None;
    }
    let rest = &target[7..];
    let (authority, path) = match rest.find(['/', '?']) {
        Some(at) if rest.as_bytes()[at] == b'/' => (&rest[..at], rest[at..].to_owned()),
        Some(at) => (&rest[..at], format!("/{}", &rest[at..])),
        None => (rest, "/".to_owned()),
    };
    // A client's own credentials in the address are not carried on.
    let authority = authority.rsplit('@').next()?;
    (!authority.is_empty()).then(|| (authority.to_owned(), path))
}

/// `host:port` (`[::1]:80` for an IPv6 literal) with `default` for a port that is not said.
fn split_authority(authority: &str, default: u16) -> Option<(String, u16)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(port) => port.parse().ok()?,
            None if after.is_empty() => default,
            None => return None,
        };
        return Some((host.to_owned(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => Some((host.to_owned(), port.parse().ok()?)),
        Some(_) => None,
        None => (!authority.is_empty()).then(|| (authority.to_owned(), default)),
    }
}

/// The request as the server it is made to is to read it: origin form, and without what is the
/// proxy's alone — the browser's identity (`Proxy-Authorization`) and `Proxy-Connection`.
fn rewritten(head: &str, method: &str, path: &str, version: &str) -> String {
    let mut out = format!("{method} {path} {version}\r\n");
    for line in head.lines().skip(1) {
        let name = line.split(':').next().unwrap_or_default().trim();
        if line.is_empty()
            || name.eq_ignore_ascii_case("proxy-authorization")
            || name.eq_ignore_ascii_case("proxy-connection")
        {
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out
}

enum Body {
    None,
    Length(u64),
    Chunked,
}

fn body_of(head: &str) -> Body {
    if header(head, "transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        return Body::Chunked;
    }
    match header(head, "content-length").and_then(|v| v.parse().ok()) {
        Some(length) if length > 0 => Body::Length(length),
        _ => Body::None,
    }
}

/// A chunked body, framing and all, from one end to the other: the proxy reads it only to know
/// where it ends.
fn copy_chunked(from: &mut TcpStream, to: &mut dyn Write) -> io::Result<()> {
    loop {
        let line = read_line(from)?;
        to.write_all(line.as_bytes())?;
        let size = line
            .trim()
            .split(';')
            .next()
            .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "a chunk with no size"))?;
        if size == 0 {
            // Trailers, to the empty line.
            loop {
                let trailer = read_line(from)?;
                to.write_all(trailer.as_bytes())?;
                if trailer.trim().is_empty() {
                    return Ok(());
                }
            }
        }
        io::copy(&mut from.take(size + 2), to)?;
    }
}

/// One line with its line end, up to [`HEAD_MAX`].
fn read_line(stream: &mut TcpStream) -> io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < HEAD_MAX {
        if stream.read(&mut byte)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        line.push(byte[0]);
        if byte[0] == b'\n' {
            return Ok(String::from_utf8_lossy(&line).into_owned());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "a line too long",
    ))
}

/// The proxy's own `502`: `why` is the sentence for the person, `code` the Chromium net error it
/// stands for, said in [`NET_ERROR`] for the helper.
fn bad_gateway(code: i32, why: &str) -> String {
    reply(
        502,
        "Bad Gateway",
        &format!("{NET_ERROR}: {code}\r\n"),
        &format!("{why}\n"),
    )
}

/// The Chromium net error a way's failure (`why`, the line the [`Dialer`] gives for the person) is:
/// told by what the line says, which is the operating system's own words for what a connection
/// fails by, or the server's.
fn net_error(why: &str) -> i32 {
    let why = why.to_ascii_lowercase();
    let says = |words: &[&str]| words.iter().any(|word| why.contains(word));
    if says(&["refused"]) {
        REFUSED
    } else if says(&["timed out", "timeout", "too many connections", "in time"]) {
        TIMED_OUT
    } else if says(&[
        "failed to lookup",
        "nodename nor servname",
        "not known",
        "name or service",
        "no such host",
        "no address",
    ]) {
        NAME_NOT_RESOLVED
    } else if says(&[
        "not running",
        "made no connection",
        "does not allow",
        "no route from",
    ]) {
        NO_WAY
    } else {
        UNREACHABLE
    }
}

fn reply(status: u16, reason: &str, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} {reason}\r\n{extra}Content-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The request's head, up to the empty line.
fn read_head(stream: &mut TcpStream) -> io::Result<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while head.len() < HEAD_MAX {
        if stream.read(&mut byte)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            return Ok(String::from_utf8_lossy(&head).into_owned());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "the request head is too long",
    ))
}

/// A header's value, by name without regard to case.
fn header(head: &str, name: &str) -> Option<String> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

/// Whether two byte strings are equal, taking as long for any pair of the same length.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut have) = (0u32, 0u32);
    for c in text.trim_end_matches('=').bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push(u8::try_from((bits >> have) & 0xff).ok()?);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(text: &str) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in text.as_bytes().chunks(3) {
            let n =
                chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
            for i in 0..=chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
            for _ in chunk.len()..3 {
                out.push('=');
            }
        }
        out
    }

    fn ask(proxy: &Proxy, head: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(head.as_bytes()).unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer
    }

    fn connect_with(user: &str, password: &str) -> String {
        format!(
            "CONNECT localhost:3000 HTTP/1.1\r\nHost: localhost:3000\r\nProxy-Authorization: Basic {}\r\n\r\n",
            encode(&format!("{user}:{password}"))
        )
    }

    #[test]
    fn base64_reads_what_a_browser_writes() {
        for text in [
            "",
            "a",
            "ab",
            "abc",
            "user:pass",
            "mem-1:0123456789abcdef",
            "ü:ö",
        ] {
            assert_eq!(
                base64_decode(&encode(text)).unwrap(),
                text.as_bytes(),
                "{text:?}"
            );
        }
        assert!(base64_decode("not base64!").is_none());
    }

    #[test]
    fn a_connection_without_credentials_gets_407_and_nothing_else() {
        let proxy = Proxy::bind(Routes::new()).unwrap();
        let key = ProfileKey::new("mac", "app", None).unwrap();
        proxy.allow("user", "secret", &key);
        for head in [
            "GET http://localhost:3000/ HTTP/1.1\r\nHost: localhost:3000\r\n\r\n".to_owned(),
            connect_with("user", "wrong"),
            connect_with("someone", "secret"),
            "CONNECT localhost:3000 HTTP/1.1\r\nProxy-Authorization: Bearer abc\r\n\r\n".to_owned(),
            "CONNECT localhost:3000 HTTP/1.1\r\nProxy-Authorization: Basic %%%\r\n\r\n".to_owned(),
        ] {
            let answer = ask(&proxy, &head);
            assert!(answer.starts_with("HTTP/1.1 407 "), "{head:?} → {answer:?}");
            assert!(answer.contains("Proxy-Authenticate: Basic"), "{answer}");
        }
    }

    #[test]
    fn a_known_identity_of_a_machine_with_no_way_is_told_so() {
        let proxy = Proxy::bind(Routes::new()).unwrap();
        let key = ProfileKey::new("mac", "app", Some("work")).unwrap();
        proxy.allow("user", "secret", &key);
        let answer = ask(&proxy, &connect_with("user", "secret"));
        assert!(answer.starts_with("HTTP/1.1 502 "), "{answer}");
        assert!(answer.contains("no route"), "{answer}");
        assert!(
            answer.contains("X-Bateri-Net-Error: -130\r\n"),
            "no way is the proxy's own failure: {answer}"
        );
    }

    #[test]
    fn it_listens_on_the_loopback_alone_and_stops_with_its_owner() {
        let proxy = Proxy::bind(Routes::new()).unwrap();
        let port = proxy.port();
        assert_eq!(proxy.url(), format!("http://127.0.0.1:{port}"));
        drop(proxy);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }

    #[test]
    fn a_head_that_never_ends_is_dropped() {
        let proxy = Proxy::bind(Routes::new()).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let junk = vec![b'x'; HEAD_MAX + 100];
        let _ = stream.write_all(&junk);
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        assert!(answer.is_empty(), "{answer:?}");
    }
}

#[cfg(test)]
mod carrying;
