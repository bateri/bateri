//! The proxy carrying requests of a known profile, against a stand-in for the way (a [`Dialer`]
//! that connects to a server of the test whatever it is asked for, and writes down what it was
//! asked) — no kablo is needed to see what the proxy does with a head, a body, a tunnel, a refusal
//! and two hundred connections at once.

use std::net::TcpListener;
use std::sync::atomic::AtomicUsize;

use super::*;
use crate::browser::route::Direct;
use crate::browser::route::Parts;
use crate::kablo::queue::{MAX_CONNECTING, MAX_OPEN, Queue};

const WAIT: Duration = Duration::from_secs(10);

fn encode(text: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
        for _ in chunk.len()..3 {
            out.push('=');
        }
    }
    out
}

fn auth(user: &str, password: &str) -> String {
    format!(
        "Proxy-Authorization: Basic {}\r\n",
        encode(&format!("{user}:{password}"))
    )
}

/// What a server of the test heard of one request.
#[derive(Clone, Debug)]
struct Heard {
    head: String,
    body: Vec<u8>,
}

/// A server that answers every request `ok` and writes down what it was sent; a connection it
/// is made to is counted.
struct Server {
    port: u16,
    heard: Arc<Mutex<Vec<Heard>>>,
    connections: Arc<AtomicUsize>,
}

impl Server {
    fn start() -> Self {
        Self::answering(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
    }

    /// A server that answers every request with `answer`.
    fn answering(answer: &'static [u8]) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let (log, count) = (heard.clone(), connections.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                count.fetch_add(1, Ordering::SeqCst);
                let log = log.clone();
                std::thread::spawn(move || {
                    let mut stream = stream;
                    while let Ok(head) = read_head(&mut stream) {
                        let body = match body_of(&head) {
                            Body::None => Vec::new(),
                            Body::Length(n) => {
                                let mut body = vec![0; usize::try_from(n).unwrap()];
                                if stream.read_exact(&mut body).is_err() {
                                    return;
                                }
                                body
                            }
                            Body::Chunked => {
                                let mut raw = Vec::new();
                                if copy_chunked(&mut stream, &mut raw).is_err() {
                                    return;
                                }
                                raw
                            }
                        };
                        lock(&log).push(Heard { head, body });
                        let _ = stream.write_all(answer);
                    }
                });
            }
        });
        Self {
            port,
            heard,
            connections,
        }
    }

    fn heard(&self) -> Vec<Heard> {
        lock(&self.heard).clone()
    }
}

/// An echo server: what a tunnel is said comes back.
fn echo() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = stream.try_clone().unwrap();
                let mut writer = stream;
                let _ = io::copy(&mut reader, &mut writer);
            });
        }
    });
    port
}

/// The way of the test: every connection goes to `to`, whatever address it was asked for, and the
/// asks are written down.
struct Way {
    to: u16,
    asked: Mutex<Vec<(String, u16)>>,
    refuse: Option<String>,
}

impl Way {
    fn new(to: u16) -> Arc<Self> {
        Arc::new(Self {
            to,
            asked: Mutex::new(Vec::new()),
            refuse: None,
        })
    }

    fn asked(&self) -> Vec<(String, u16)> {
        lock(&self.asked).clone()
    }
}

impl Dialer for Way {
    fn dial(&self, host: &str, port: u16, wait: Duration) -> Result<Link, String> {
        lock(&self.asked).push((host.to_owned(), port));
        if let Some(why) = &self.refuse {
            return Err(why.clone());
        }
        Direct.dial("127.0.0.1", self.to, wait)
    }
}

struct Rig {
    proxy: Proxy,
    routes: Arc<Routes>,
}

fn rig(machine: &str) -> (Rig, ProfileKey) {
    let routes = Routes::new();
    let proxy = Proxy::bind(routes.clone()).unwrap();
    let key = ProfileKey::new(machine, "app", None).unwrap();
    proxy.allow("user", "secret", &key);
    (Rig { proxy, routes }, key)
}

impl Rig {
    fn client(&self) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", self.proxy.port())).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream
    }
}

/// A response: its status line and what follows the head, by its `Content-Length`.
fn response(stream: &mut TcpStream) -> (String, String) {
    let head = read_head(stream).expect("a response");
    let length: usize = header(&head, "content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    (head, String::from_utf8(body).unwrap())
}

#[test]
fn a_connect_is_a_tunnel_to_the_address_it_names() {
    let (rig, _) = rig("mac");
    let way = Way::new(echo());
    rig.routes.set("mac", way.clone());
    let mut client = rig.client();
    client
        .write_all(
            format!(
                "CONNECT dev.test:3000 HTTP/1.1\r\nHost: dev.test:3000\r\n{}\r\n",
                auth("user", "secret")
            )
            .as_bytes(),
        )
        .unwrap();
    let head = read_head(&mut client).unwrap();
    assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
    client.write_all(b"ping").unwrap();
    let mut back = [0u8; 4];
    client.read_exact(&mut back).unwrap();
    assert_eq!(&back, b"ping");
    assert_eq!(way.asked(), vec![("dev.test".to_owned(), 3000)]);
}

#[test]
fn a_request_with_an_absolute_address_is_made_there_as_the_server_reads_it() {
    let (rig, _) = rig("mac");
    let server = Server::start();
    let way = Way::new(server.port);
    rig.routes.set("mac", way.clone());
    let mut client = rig.client();
    client
        .write_all(
            format!(
                "GET http://localhost:3000/a/b?q=1 HTTP/1.1\r\nHost: localhost:3000\r\n{}Proxy-Connection: keep-alive\r\nAccept: */*\r\n\r\n",
                auth("user", "secret")
            )
            .as_bytes(),
        )
        .unwrap();
    let (head, body) = response(&mut client);
    assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
    assert_eq!(body, "ok");
    assert_eq!(way.asked(), vec![("localhost".to_owned(), 3000)]);
    let heard = server.heard();
    assert_eq!(heard.len(), 1);
    let sent = &heard[0].head;
    assert!(sent.starts_with("GET /a/b?q=1 HTTP/1.1\r\n"), "{sent}");
    assert!(sent.contains("Host: localhost:3000"), "{sent}");
    assert!(sent.contains("Accept: */*"), "{sent}");
    // The browser's identity does not reach the server it asks.
    assert!(!sent.to_lowercase().contains("proxy-"), "{sent}");
    assert!(!sent.contains(&encode("user:secret")), "{sent}");
}

#[test]
fn the_connection_to_an_address_is_kept_for_the_next_request_and_another_address_is_another() {
    let (rig, _) = rig("mac");
    let server = Server::start();
    let way = Way::new(server.port);
    rig.routes.set("mac", way.clone());
    let mut client = rig.client();
    let get = |authority: &str, path: &str| {
        format!(
            "GET http://{authority}{path} HTTP/1.1\r\nHost: {authority}\r\n{}\r\n",
            auth("user", "secret")
        )
    };
    for request in [
        get("a.test:80", "/1"),
        get("a.test:80", "/2"),
        get("b.test:8080", "/3"),
        get("a.test:80", "/4"),
    ] {
        client.write_all(request.as_bytes()).unwrap();
        assert_eq!(response(&mut client).1, "ok");
    }
    // a.test once (kept for /2 and /4), b.test once.
    assert_eq!(
        way.asked(),
        vec![("a.test".to_owned(), 80), ("b.test".to_owned(), 8080)]
    );
    assert_eq!(server.connections.load(Ordering::SeqCst), 2);
    assert_eq!(server.heard().len(), 4);
}

#[test]
fn a_body_goes_on_whole_by_its_length_or_in_chunks() {
    let (rig, _) = rig("mac");
    let server = Server::start();
    rig.routes.set("mac", Way::new(server.port));
    let mut client = rig.client();
    client
        .write_all(
            format!(
                "POST http://x.test/length HTTP/1.1\r\nHost: x.test\r\n{}Content-Length: 11\r\n\r\nhello world",
                auth("user", "secret")
            )
            .as_bytes(),
        )
        .unwrap();
    assert_eq!(response(&mut client).1, "ok");
    client
        .write_all(
            format!(
                "POST http://x.test/chunks HTTP/1.1\r\nHost: x.test\r\n{}Transfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\nTrailer: x\r\n\r\n",
                auth("user", "secret")
            )
            .as_bytes(),
        )
        .unwrap();
    assert_eq!(response(&mut client).1, "ok");
    // Something after the body is a request of its own.
    client
        .write_all(
            format!(
                "GET http://x.test/after HTTP/1.1\r\nHost: x.test\r\n{}\r\n",
                auth("user", "secret")
            )
            .as_bytes(),
        )
        .unwrap();
    assert_eq!(response(&mut client).1, "ok");
    let heard = server.heard();
    assert_eq!(heard.len(), 3);
    assert_eq!(heard[0].body, b"hello world");
    assert_eq!(
        String::from_utf8(heard[1].body.clone()).unwrap(),
        "5\r\nhello\r\n6;ext=1\r\n world\r\n0\r\nTrailer: x\r\n\r\n"
    );
    assert!(heard[2].head.starts_with("GET /after "));
}

#[test]
fn a_connection_the_way_does_not_make_is_a_502_that_says_why() {
    let (rig, _) = rig("mac");
    let way = Arc::new(Way {
        to: 1,
        asked: Mutex::new(Vec::new()),
        refuse: Some(
            "the server does not allow connections to be forwarded (no-forwarding)".into(),
        ),
    });
    rig.routes.set("mac", way);
    for request in [
        format!(
            "CONNECT dev.test:443 HTTP/1.1\r\n{}\r\n",
            auth("user", "secret")
        ),
        format!(
            "GET http://dev.test/ HTTP/1.1\r\nHost: dev.test\r\n{}\r\n",
            auth("user", "secret")
        ),
    ] {
        let mut client = rig.client();
        client.write_all(request.as_bytes()).unwrap();
        let (head, body) = response(&mut client);
        assert!(head.starts_with("HTTP/1.1 502 "), "{head}");
        assert!(body.contains("no-forwarding"), "{body}");
        // A way that does not allow it is no way: the helper is told so.
        assert_eq!(
            header(&head, "x-bateri-net-error").as_deref(),
            Some("-130"),
            "{head}"
        );
    }
}

/// What the helper is told when a connection fails, by the words the way gives: the page's load
/// error is the Chromium error it stands for, for a plain request and for a tunnel alike.
#[test]
fn each_failure_of_a_connection_is_the_net_error_it_stands_for() {
    for (why, code) in [
        ("localhost:3000: Connection refused (os error 61)", "-102"),
        ("localhost:3000: connection timed out", "-118"),
        (
            "too many connections are open through the server just now",
            "-118",
        ),
        (
            "nope.test: failed to lookup address information: nodename nor servname provided, or not known",
            "-105",
        ),
        ("nope.test:80: no address", "-105"),
        ("bateri's connections are not running", "-130"),
        ("localhost:3000: No route to host (os error 65)", "-109"),
        ("something nobody foresaw", "-109"),
    ] {
        let (rig, _) = rig("mac");
        rig.routes.set(
            "mac",
            Arc::new(Way {
                to: 1,
                asked: Mutex::new(Vec::new()),
                refuse: Some(why.into()),
            }),
        );
        for request in [
            format!(
                "GET http://dev.test/ HTTP/1.1\r\nHost: dev.test\r\n{}\r\n",
                auth("user", "secret")
            ),
            format!(
                "CONNECT dev.test:443 HTTP/1.1\r\n{}\r\n",
                auth("user", "secret")
            ),
        ] {
            let mut client = rig.client();
            client.write_all(request.as_bytes()).unwrap();
            let (head, body) = response(&mut client);
            assert!(head.starts_with("HTTP/1.1 502 "), "{head}");
            assert_eq!(
                header(&head, "x-bateri-net-error").as_deref(),
                Some(code),
                "{why}: {head}"
            );
            assert!(body.contains(why), "the person's sentence stays: {body}");
        }
    }
}

/// A connection the program's machine refuses, made for real: the engine's own words for it are
/// what says so.
#[test]
fn a_port_nothing_listens_on_is_a_refused_connection_to_the_helper() {
    let closed = {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    };
    let (rig, _) = rig("mac");
    rig.routes.set("mac", Way::new(closed));
    let mut client = rig.client();
    client
        .write_all(
            format!(
                "GET http://dev.test/ HTTP/1.1\r\nHost: dev.test\r\n{}\r\n",
                auth("user", "secret")
            )
            .as_bytes(),
        )
        .unwrap();
    let (head, _) = response(&mut client);
    assert!(head.starts_with("HTTP/1.1 502 "), "{head}");
    assert_eq!(
        header(&head, "x-bateri-net-error").as_deref(),
        Some("-102"),
        "{head}"
    );
}

/// The header is the proxy's to say: a server that sends it (to make a page look failed) is
/// carried as it is without it, whatever else it sends.
#[test]
fn a_servers_answer_with_the_proxys_header_is_carried_without_it() {
    let server = Server::answering(
        b"HTTP/1.1 502 Bad Gateway\r\nX-Bateri-Net-Error: -102\r\nContent-Length: 2\r\n\r\nok",
    );
    let (rig, _) = rig("mac");
    rig.routes.set("mac", Way::new(server.port));
    let mut client = rig.client();
    // Two requests on one connection: both answers lose it.
    for _ in 0..2 {
        client
            .write_all(
                format!(
                    "GET http://dev.test/ HTTP/1.1\r\nHost: dev.test\r\n{}\r\n",
                    auth("user", "secret")
                )
                .as_bytes(),
            )
            .unwrap();
        let (head, body) = response(&mut client);
        assert!(head.starts_with("HTTP/1.1 502 "), "{head}");
        assert_eq!(header(&head, "x-bateri-net-error"), None, "{head}");
        assert_eq!(body, "ok");
    }
}

#[test]
fn each_profile_goes_by_the_way_of_its_machine_and_a_machine_without_one_by_none() {
    let routes = Routes::new();
    let proxy = Proxy::bind(routes.clone()).unwrap();
    let (a, b, c) = (Server::start(), Server::start(), Server::start());
    let (way_a, way_b) = (Way::new(a.port), Way::new(b.port));
    routes.set("one", way_a.clone());
    routes.set("two", way_b.clone());
    for (user, machine) in [("ua", "one"), ("ub", "two"), ("uc", "three")] {
        proxy.allow(user, "pw", &ProfileKey::new(machine, "app", None).unwrap());
    }
    for (user, expect) in [("ua", "200"), ("ub", "200"), ("uc", "502")] {
        let mut stream = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream
            .write_all(
                format!(
                    "GET http://localhost:3000/ HTTP/1.1\r\nHost: localhost:3000\r\n{}\r\n",
                    auth(user, "pw")
                )
                .as_bytes(),
            )
            .unwrap();
        let (head, _) = response(&mut stream);
        assert!(head.contains(expect), "{user}: {head}");
    }
    assert_eq!(
        (a.heard().len(), b.heard().len(), c.heard().len()),
        (1, 1, 0)
    );
    assert_eq!((way_a.asked().len(), way_b.asked().len()), (1, 1));
}

#[test]
fn a_connection_does_not_change_identity_in_the_middle() {
    let (rig, key) = rig("mac");
    rig.proxy.allow("other", "pw", &key);
    let server = Server::start();
    rig.routes.set("mac", Way::new(server.port));
    let mut client = rig.client();
    let get = |who: &str, password: &str| {
        format!(
            "GET http://x.test/ HTTP/1.1\r\nHost: x.test\r\n{}\r\n",
            auth(who, password)
        )
    };
    client.write_all(get("user", "secret").as_bytes()).unwrap();
    assert_eq!(response(&mut client).1, "ok");
    // The same profile under another name is another identity as far as a connection goes.
    client.write_all(get("user", "wrong").as_bytes()).unwrap();
    let (head, _) = response(&mut client);
    assert!(head.starts_with("HTTP/1.1 407 "), "{head}");
    assert_eq!(server.heard().len(), 1);
}

#[test]
fn what_is_not_a_request_to_an_address_is_refused() {
    let (rig, _) = rig("mac");
    rig.routes.set("mac", Way::new(1));
    for target in ["/path", "ftp://x.test/", "https://x.test/"] {
        let mut client = rig.client();
        client
            .write_all(
                format!(
                    "GET {target} HTTP/1.1\r\nHost: x\r\n{}\r\n",
                    auth("user", "secret")
                )
                .as_bytes(),
            )
            .unwrap();
        let (head, _) = response(&mut client);
        assert!(head.starts_with("HTTP/1.1 400 "), "{target}: {head}");
    }
}

#[test]
fn addresses_and_requests_are_taken_apart() {
    assert_eq!(
        split_target("http://localhost:3000/a?b=1"),
        Some(("localhost:3000".into(), "/a?b=1".into()))
    );
    assert_eq!(split_target("http://h"), Some(("h".into(), "/".into())));
    assert_eq!(
        split_target("http://h?x=1"),
        Some(("h".into(), "/?x=1".into()))
    );
    assert_eq!(
        split_target("HTTP://user:pw@h:81/p"),
        Some(("h:81".into(), "/p".into()))
    );
    assert_eq!(split_target("/p"), None);
    assert_eq!(split_authority("h", 80), Some(("h".into(), 80)));
    assert_eq!(split_authority("h:8080", 80), Some(("h".into(), 8080)));
    assert_eq!(
        split_authority("[::1]:3000", 80),
        Some(("::1".into(), 3000))
    );
    assert_eq!(split_authority("[::1]", 80), Some(("::1".into(), 80)));
    assert_eq!(split_authority("h:notaport", 80), None);
    assert_eq!(split_authority(":80", 80), None);
}

/// Kablo in miniature: it takes 16 connecting and 128 open and refuses the rest, and what it takes
/// goes to a server of the test. The proxy's way waits in the line in front of it, as the way of a
/// real server does ([`crate::kablo::served::Served::connection`]).
struct Kablo {
    line: Arc<Queue>,
    to: u16,
    connecting: AtomicUsize,
    open: Arc<AtomicUsize>,
    refused: AtomicUsize,
}

/// A stream kablo holds open, counted until it is dropped.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Dialer for Kablo {
    fn dial(&self, _: &str, _: u16, wait: Duration) -> Result<Link, String> {
        let mut place = self.line.enter(wait).ok_or("no place in the line")?;
        let now = self.connecting.fetch_add(1, Ordering::SeqCst) + 1;
        let held = self.open.fetch_add(1, Ordering::SeqCst) + 1;
        let counted = Counted(Arc::clone(&self.open));
        if now > 16 || held > 128 {
            self.refused.fetch_add(1, Ordering::SeqCst);
        }
        std::thread::sleep(Duration::from_millis(3));
        self.connecting.fetch_sub(1, Ordering::SeqCst);
        place.connected();
        let link = Direct.dial("127.0.0.1", self.to, wait)?;
        let Parts {
            reader,
            writer,
            end,
            ..
        } = link.split();
        Ok(Link::new(
            reader,
            writer,
            move || end(),
            Some(Box::new((place, counted))),
        ))
    }
}

#[test]
fn two_hundred_connections_at_once_pass_in_turn_and_the_way_refuses_none() {
    let (rig, _) = rig("mac");
    let server = Server::start();
    let kablo = Arc::new(Kablo {
        line: Queue::new(MAX_CONNECTING, MAX_OPEN),
        to: server.port,
        connecting: AtomicUsize::new(0),
        open: Arc::new(AtomicUsize::new(0)),
        refused: AtomicUsize::new(0),
    });
    rig.routes.set("mac", kablo.clone());
    let port = rig.proxy.port();
    let threads: Vec<_> = (0..200)
        .map(|n| {
            std::thread::spawn(move || {
                let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
                client.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
                client
                    .write_all(
                        format!(
                            "GET http://dev.test:3000/{n} HTTP/1.1\r\nHost: dev.test:3000\r\n{}\r\n",
                            auth("user", "secret")
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                response(&mut client)
            })
        })
        .collect();
    for thread in threads {
        let (head, body) = thread.join().unwrap();
        assert!(head.starts_with("HTTP/1.1 200 "), "{head}");
        assert_eq!(body, "ok");
    }
    assert_eq!(server.heard().len(), 200);
    // What kablo would have refused is what the line was for.
    assert_eq!(kablo.refused.load(Ordering::SeqCst), 0);
}
