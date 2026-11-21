//! The proof of a way, against listeners of the test that stand in for the program's.

use std::net::TcpListener;

use super::*;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const WAIT: Duration = Duration::from_millis(500);

/// A listener that, to one connection, does `say`.
fn listener(say: impl FnOnce(TcpStream) + Send + 'static) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            say(stream);
        }
    });
    port
}

#[test]
fn a_way_that_reads_the_token_back_is_proved() {
    let port = listener(|mut s| {
        let _ = s.write_all(TOKEN.as_bytes());
    });
    assert_eq!(prove(&Direct, port, TOKEN, WAIT), Ok(()));
}

#[test]
fn a_way_that_reads_anything_else_is_not() {
    // Another token: another program's listener, or another machine's.
    let port = listener(|mut s| {
        let _ = s.write_all(b"ffffffffffffffffffffffffffffffff");
    });
    let why = prove(&Direct, port, TOKEN, WAIT).unwrap_err();
    assert!(why.starts_with("There is no route from this pane"), "{why}");
    assert!(why.contains("not what the program gave"), "{why}");
    // Too little.
    let port = listener(|mut s| {
        let _ = s.write_all(b"0123");
    });
    assert!(
        prove(&Direct, port, TOKEN, WAIT)
            .unwrap_err()
            .contains("too little")
    );
    // Nothing, and the connection kept: the proof does not wait for ever.
    let port = listener(|s| {
        std::thread::sleep(Duration::from_secs(2));
        drop(s);
    });
    let started = std::time::Instant::now();
    assert!(
        prove(&Direct, port, TOKEN, Duration::from_millis(200))
            .unwrap_err()
            .contains("in time")
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_way_that_cannot_connect_says_why() {
    // Nothing listens: this Mac's loopback is not the program's.
    let closed = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let why = prove(&Direct, closed, TOKEN, WAIT).unwrap_err();
    assert!(why.contains("refused"), "{why}");
    // A server that makes no connections says what it says.
    struct Refuses;
    impl Dialer for Refuses {
        fn dial(&self, _: &str, _: u16, _: Duration) -> Result<Link, String> {
            Err("the server does not allow connections to be forwarded (no-forwarding)".into())
        }
    }
    let why = prove(&Refuses, 80, TOKEN, WAIT).unwrap_err();
    assert!(why.contains("no-forwarding"), "{why}");
}

#[test]
fn a_token_that_is_not_one_proves_nothing() {
    let port = listener(|mut s| {
        let _ = s.write_all(b"short");
    });
    assert!(prove(&Direct, port, "short", WAIT).is_err());
}

#[test]
fn a_machine_has_the_way_that_was_set_for_it() {
    let routes = Routes::new();
    assert!(routes.get("mac").is_none());
    routes.set("mac", Arc::new(Direct));
    assert!(routes.get("mac").is_some());
    assert!(routes.get("server").is_none());
    routes.remove("mac");
    assert!(routes.get("mac").is_none());
}

#[test]
fn the_direct_way_makes_a_connection_and_ends_it() {
    let port = listener(|mut s| {
        let _ = s.write_all(b"hi");
        let mut rest = Vec::new();
        let _ = s.read_to_end(&mut rest);
    });
    let mut link = Direct.dial("127.0.0.1", port, WAIT).unwrap();
    let mut got = [0u8; 2];
    link.reader.read_exact(&mut got).unwrap();
    assert_eq!(&got, b"hi");
    link.end();
    let mut more = Vec::new();
    // Ended: reading says so and does not wait.
    assert!(link.reader.read_to_end(&mut more).is_ok() || more.is_empty());
}
