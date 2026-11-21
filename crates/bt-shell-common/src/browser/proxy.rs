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
//! The request of a profile that is known is not carried yet (Phase 6 of plan 034 puts the route
//! behind it): it is answered `502` with the reason.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::profile::ProfileKey;

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

#[derive(Default)]
struct Registry {
    identities: Mutex<HashMap<String, Identity>>,
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
    /// Starts listening on a port of the loopback the system chooses.
    pub fn bind() -> io::Result<Proxy> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let registry = Arc::new(Registry::default());
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
    let answer = match known {
        None => reply(
            407,
            "Proxy Authentication Required",
            "Proxy-Authenticate: Basic realm=\"bateri\"\r\n",
            "",
        ),
        Some(_) => reply(
            502,
            "Bad Gateway",
            "",
            "There is no route from this pane to the program's machine yet.\n",
        ),
    };
    stream.write_all(answer.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
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
        let proxy = Proxy::bind().unwrap();
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
    fn a_known_identity_is_let_past_the_door() {
        let proxy = Proxy::bind().unwrap();
        let key = ProfileKey::new("mac", "app", Some("work")).unwrap();
        proxy.allow("user", "secret", &key);
        let answer = ask(&proxy, &connect_with("user", "secret"));
        assert!(answer.starts_with("HTTP/1.1 502 "), "{answer}");
        assert!(answer.contains("no route"), "{answer}");
    }

    #[test]
    fn it_listens_on_the_loopback_alone_and_stops_with_its_owner() {
        let proxy = Proxy::bind().unwrap();
        let port = proxy.port();
        assert_eq!(proxy.url(), format!("http://127.0.0.1:{port}"));
        drop(proxy);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    }

    #[test]
    fn a_head_that_never_ends_is_dropped() {
        let proxy = Proxy::bind().unwrap();
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
