//! What a server's answers are on their way back to the browser: the header that says a request
//! failed on the way is the proxy's alone.
//!
//! The proxy tells the helper that a page's document could not be fetched with `X-Bateri-Net-Error`
//! on its own `502` ([`super::NET_ERROR`]); the helper takes the header from a `502` and trusts
//! it. A server the request was made to must not be able to say the same, so the answers of a plain
//! `http` connection are read as they pass: heads (and only heads) lose the header, and the body is
//! carried as it is, by the framing its head says. What cannot be read as an answer (not a head,
//! a head too large, a chunk size that is none) is carried unread to the end of the connection:
//! the proxy never stops a page from loading for a header.
//!
//! A `CONNECT` tunnel is the browser's own conversation with the server in encryption, which the
//! proxy does not see and cannot touch.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::Mutex;

use super::{HEAD_MAX, NET_ERROR, lock};

/// The largest answer head carried as one: a bigger one is carried as it came.
const HEAD_LIMIT: usize = HEAD_MAX * 4;

/// The requests sent on a connection that have no final answer yet, oldest first: `true` for a
/// `HEAD`, whose answer has the head of a body and none.
pub(super) type Asking = Mutex<VecDeque<bool>>;

/// Carries the answers of `from` to `to` until `from` has no more, without the proxy's header.
pub(super) fn carry(from: impl Read, to: &mut impl Write, asking: &Asking) -> io::Result<()> {
    let mut from = BufReader::new(from);
    loop {
        let mut head = Vec::new();
        match read_head(&mut from, &mut head)? {
            Head::Whole => {}
            // The end, with whatever was read of a head that did not end.
            Head::Ended => return to.write_all(&head),
            Head::Not => return rest(&head, &mut from, to),
        }
        let Some(answer) = Answer::of(&head) else {
            return rest(&head, &mut from, to);
        };
        to.write_all(&answer.head)?;
        // The connection is the two ends' own after this: nothing here is an answer.
        if answer.status == 101 {
            return rest(&[], &mut from, to);
        }
        // An interim answer is followed by the one it is the interim of.
        if (100..200).contains(&answer.status) {
            continue;
        }
        let head_request = lock(asking).pop_front().unwrap_or(false);
        if head_request || matches!(answer.status, 204 | 304) {
            continue;
        }
        if answer.chunked {
            if chunks(&mut from, to)? {
                return Ok(());
            }
        } else if let Some(length) = answer.length {
            let sent = io::copy(&mut Read::by_ref(&mut from).take(length), to)?;
            if sent < length {
                return Ok(());
            }
        } else {
            // What is left is this answer's body, to the end of the connection.
            return rest(&[], &mut from, to);
        }
    }
}

/// What `head` and the rest of the connection are, carried as they are to the end.
fn rest(head: &[u8], from: &mut impl Read, to: &mut impl Write) -> io::Result<()> {
    to.write_all(head)?;
    io::copy(from, to).map(|_| ())
}

enum Head {
    /// A head up to its empty line, in the bytes.
    Whole,
    /// The connection ended; the bytes are what came of a head that did not end.
    Ended,
    /// Not a head: too large, or no answer's first line.
    Not,
}

/// Reads one head, with its line ends, into `head`.
fn read_head(from: &mut impl BufRead, head: &mut Vec<u8>) -> io::Result<Head> {
    loop {
        let before = head.len();
        if line(from, head, HEAD_LIMIT)? == 0 {
            return Ok(Head::Ended);
        }
        if head.len() > HEAD_LIMIT {
            return Ok(Head::Not);
        }
        if before == 0 {
            // The first line says whether this is an answer at all.
            if !head.starts_with(b"HTTP/") {
                return Ok(Head::Not);
            }
        } else if matches!(&head[before..], b"\r\n" | b"\n") {
            return Ok(Head::Whole);
        }
    }
}

/// One line of at most `limit` bytes (and one more, which says it was too long) into `into`.
fn line(from: &mut impl BufRead, into: &mut Vec<u8>, limit: usize) -> io::Result<usize> {
    from.by_ref().take(limit as u64 + 1).read_until(b'\n', into)
}

/// An answer's head, read: what it is carried on by, and the head to carry.
struct Answer {
    status: u16,
    length: Option<u64>,
    chunked: bool,
    /// The head as it is sent on: the proxy's header out.
    head: Vec<u8>,
}

impl Answer {
    fn of(raw: &[u8]) -> Option<Answer> {
        let mut lines = raw.split_inclusive(|byte| *byte == b'\n');
        let first = lines.next()?;
        let status: u16 = std::str::from_utf8(first)
            .ok()?
            .split(' ')
            .nth(1)?
            .trim()
            .parse()
            .ok()?;
        let mut answer = Answer {
            status,
            length: None,
            chunked: false,
            head: first.to_vec(),
        };
        // A header of the proxy's, with the lines folded under it (a line that begins with
        // blank), is dropped.
        let mut dropping = false;
        for line in lines {
            if matches!(line.first(), Some(b' ' | b'\t')) {
                if !dropping {
                    answer.head.extend_from_slice(line);
                }
                continue;
            }
            dropping = false;
            if let Some(colon) = line.iter().position(|byte| *byte == b':') {
                let name = String::from_utf8_lossy(&line[..colon]);
                let name = name.trim();
                let value = String::from_utf8_lossy(&line[colon + 1..]);
                let value = value.trim();
                if name.eq_ignore_ascii_case(NET_ERROR) {
                    dropping = true;
                    continue;
                }
                if name.eq_ignore_ascii_case("content-length") {
                    answer.length = answer.length.or_else(|| value.parse().ok());
                } else if name.eq_ignore_ascii_case("transfer-encoding")
                    && value.to_ascii_lowercase().contains("chunked")
                {
                    answer.chunked = true;
                }
            }
            answer.head.extend_from_slice(line);
        }
        Some(answer)
    }
}

/// A chunked body with its framing and trailers, to its end. `true` when the connection ended in
/// it or what was read is no framing (the rest has been carried as it is).
fn chunks(from: &mut BufReader<impl Read>, to: &mut impl Write) -> io::Result<bool> {
    loop {
        let mut line = Vec::new();
        if self::line(from, &mut line, HEAD_MAX)? == 0 {
            return Ok(true);
        }
        let size = std::str::from_utf8(&line)
            .ok()
            .and_then(|text| text.trim().split(';').next())
            .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok());
        let Some(size) = size.filter(|_| line.ends_with(b"\n")) else {
            rest(&line, from, to)?;
            return Ok(true);
        };
        to.write_all(&line)?;
        if size == 0 {
            // Trailers, to the empty line.
            loop {
                let mut trailer = Vec::new();
                if self::line(from, &mut trailer, HEAD_MAX)? == 0 {
                    return Ok(true);
                }
                to.write_all(&trailer)?;
                if matches!(trailer.as_slice(), b"\r\n" | b"\n") {
                    return Ok(false);
                }
            }
        }
        // The chunk and its line end.
        let want = size + 2;
        let sent = io::copy(&mut Read::by_ref(from).take(want), to)?;
        if sent < want {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn through(from: &[u8], asked: &[bool]) -> Vec<u8> {
        let asking: Asking = Mutex::new(asked.iter().copied().collect());
        let mut out = Vec::new();
        carry(from, &mut out, &asking).unwrap();
        out
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn the_proxys_header_is_not_a_servers_to_say() {
        let answer =
            b"HTTP/1.1 502 Bad Gateway\r\nx-bateri-net-error: -102\r\nContent-Length: 2\r\n\
            X-BATERI-NET-ERROR : -7\r\n  folded\r\nServer: s\r\n\r\nok";
        let out = text(&through(answer, &[false]));
        assert_eq!(
            out,
            "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 2\r\nServer: s\r\n\r\nok"
        );
    }

    #[test]
    fn a_body_is_carried_as_it_is_even_where_it_names_the_header() {
        let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 27\r\n\r\nX-Bateri-Net-Error: -102\r\nx";
        let out = through(answer, &[false]);
        assert_eq!(out, answer);
    }

    #[test]
    fn answers_one_after_another_on_a_connection_each_lose_the_header() {
        let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc\
            HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nX-Bateri-Net-Error: 1\r\n\r\n\
            3\r\ndef\r\n0\r\n\r\n\
            HTTP/1.1 502 Bad Gateway\r\nX-Bateri-Net-Error: -102\r\nContent-Length: 1\r\n\r\nz";
        let out = text(&through(answer, &[false, false, false]));
        assert_eq!(
            out,
            "HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc\
            HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
            3\r\ndef\r\n0\r\n\r\n\
            HTTP/1.1 502 Bad Gateway\r\nContent-Length: 1\r\n\r\nz"
        );
    }

    #[test]
    fn what_has_no_body_has_none_to_carry() {
        // The answer to a HEAD has the length of a body that is not there; a 204 and a 304 have
        // none either: the next head is read where it begins.
        let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n\
            HTTP/1.1 204 No Content\r\nContent-Length: 7\r\n\r\n\
            HTTP/1.1 304 Not Modified\r\n\r\n\
            HTTP/1.1 502 Bad Gateway\r\nX-Bateri-Net-Error: -102\r\nContent-Length: 0\r\n\r\n";
        let out = text(&through(answer, &[true, false, false, false]));
        assert_eq!(
            out,
            "HTTP/1.1 200 OK\r\nContent-Length: 500\r\n\r\n\
            HTTP/1.1 204 No Content\r\nContent-Length: 7\r\n\r\n\
            HTTP/1.1 304 Not Modified\r\n\r\n\
            HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n"
        );
    }

    #[test]
    fn an_interim_answer_is_not_the_answer_to_a_request() {
        let answer = b"HTTP/1.1 100 Continue\r\nX-Bateri-Net-Error: 1\r\n\r\n\
            HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx\
            HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\ny";
        // One request is asked: the second answer has none, and still is read by its length.
        let out = text(&through(answer, &[false]));
        assert_eq!(
            out,
            "HTTP/1.1 100 Continue\r\n\r\n\
            HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx\
            HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\ny"
        );
    }

    #[test]
    fn what_follows_an_upgrade_and_a_body_with_no_end_is_carried_unread() {
        let upgrade = b"HTTP/1.1 101 Switching Protocols\r\nX-Bateri-Net-Error: 1\r\n\r\n\
            HTTP/1.1 502 not a head, but the stream's own";
        assert_eq!(
            text(&through(upgrade, &[false])),
            "HTTP/1.1 101 Switching Protocols\r\n\r\n\
            HTTP/1.1 502 not a head, but the stream's own"
        );
        // No length and no chunks: the body is what comes until the connection ends.
        let open = b"HTTP/1.0 200 OK\r\nServer: s\r\n\r\nX-Bateri-Net-Error: 1\r\nmore";
        assert_eq!(through(open, &[false]), open);
    }

    #[test]
    fn what_is_no_answer_is_carried_as_it_came() {
        for odd in [
            &b"garbage that is no head at all\r\n\r\n"[..],
            b"HTTP/1.1 nine hundred\r\nX-Bateri-Net-Error: 1\r\n\r\nbody",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nX-Bateri-Net-Error: 1\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Le",
            b"",
        ] {
            assert_eq!(through(odd, &[false]), odd, "{}", text(odd));
        }
    }

    #[test]
    fn a_connection_that_ends_in_a_body_ends_the_carrying() {
        let out = through(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort",
            &[false],
        );
        assert_eq!(out, b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort");
        let out = through(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab",
            &[false],
        );
        assert_eq!(
            out,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab"
        );
    }
}
