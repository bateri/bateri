//! What a server's answers are on their way back to the browser: the header that says a request
//! failed on the way is the proxy's alone.
//!
//! The proxy tells the helper that a page's document could not be fetched with `X-Bateri-Net-Error`
//! on its own `502` ([`super::NET_ERROR`]); the helper takes the header from a `502` and trusts
//! it. A server the request was made to must not be able to say the same, so the answers of a plain
//! `http` connection are read as they pass: heads (and only heads) lose the header, and the body is
//! carried as it is, by the framing its head says. What cannot be read as an answer (a head that
//! is not one, too large, cut short, a status that is none, a chunk size that is none) is not
//! carried and ends the connection: the browser could make nothing of it either, and a header the
//! proxy could not look for must not reach it. Only what follows an upgrade, and a body with no
//! end but the connection's, is carried unread.
//!
//! A `CONNECT` tunnel is the browser's own conversation with the server in encryption, which the
//! proxy does not see and cannot touch.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::Mutex;

use super::{HEAD_MAX, NET_ERROR, lock};

/// The largest answer head carried: a bigger one ends the connection.
const HEAD_LIMIT: usize = HEAD_MAX * 4;

/// The requests sent on a connection that have no final answer yet, oldest first: `true` for a
/// `HEAD`, whose answer has the head of a body and none.
pub(super) type Asking = Mutex<VecDeque<bool>>;

/// Carries the answers of `from` to `to` until `from` has no more, without the proxy's header.
pub(super) fn carry(from: impl Read, to: &mut impl Write, asking: &Asking) -> io::Result<()> {
    let mut from = BufReader::new(from);
    loop {
        let mut head = Vec::new();
        // The end of the connection, or something that is no answer: nothing more is carried.
        let Head::Whole = read_head(&mut from, &mut head)? else {
            return Ok(());
        };
        let Some(answer) = Answer::of(&head) else {
            return Ok(());
        };
        to.write_all(&answer.head)?;
        // The connection is the two ends' own after this: nothing here is an answer.
        if answer.status == 101 {
            return rest(&mut from, to);
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
            if !chunks(&mut from, to)? {
                return Ok(());
            }
        } else if let Some(length) = answer.length {
            let sent = io::copy(&mut Read::by_ref(&mut from).take(length), to)?;
            if sent < length {
                return Ok(());
            }
        } else {
            // What is left is this answer's body, to the end of the connection.
            return rest(&mut from, to);
        }
    }
}

/// The rest of the connection, carried as it is to the end.
fn rest(from: &mut impl Read, to: &mut impl Write) -> io::Result<()> {
    io::copy(from, to).map(|_| ())
}

enum Head {
    /// A head up to its empty line, in the bytes.
    Whole,
    /// The connection ended (maybe in a head that did not), or what came is not a head: too large,
    /// or its first line is no answer's.
    No,
}

/// Reads one head, with its line ends, into `head`.
fn read_head(from: &mut impl BufRead, head: &mut Vec<u8>) -> io::Result<Head> {
    loop {
        let before = head.len();
        if line(from, head, HEAD_LIMIT)? == 0 || head.len() > HEAD_LIMIT {
            return Ok(Head::No);
        }
        if before == 0 {
            // The first line says whether this is an answer at all.
            if !head.starts_with(b"HTTP/") {
                return Ok(Head::No);
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
        // `HTTP/1.1`, blanks, then the status's digits (the engine's own reading: blanks of any
        // number or kind, and what follows the digits is the reason).
        let after_version = first.iter().position(|byte| matches!(byte, b' ' | b'\t'))?;
        let digits: Vec<u8> = first[after_version..]
            .iter()
            .skip_while(|byte| matches!(byte, b' ' | b'\t'))
            .take_while(|byte| byte.is_ascii_digit())
            .copied()
            .collect();
        let status: u16 = std::str::from_utf8(&digits).ok()?.parse().ok()?;
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

/// A chunked body with its framing and trailers, to its end. `false` when the connection ended in
/// it or what was read is no framing (nothing more is carried).
fn chunks(from: &mut BufReader<impl Read>, to: &mut impl Write) -> io::Result<bool> {
    loop {
        let mut line = Vec::new();
        if self::line(from, &mut line, HEAD_MAX)? == 0 {
            return Ok(false);
        }
        let size = std::str::from_utf8(&line)
            .ok()
            .and_then(|text| text.trim().split(';').next())
            .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok());
        let Some(size) = size.filter(|_| line.ends_with(b"\n")) else {
            return Ok(false);
        };
        to.write_all(&line)?;
        if size == 0 {
            // Trailers, to the empty line.
            loop {
                let mut trailer = Vec::new();
                if self::line(from, &mut trailer, HEAD_MAX)? == 0 {
                    return Ok(false);
                }
                to.write_all(&trailer)?;
                if matches!(trailer.as_slice(), b"\r\n" | b"\n") {
                    return Ok(true);
                }
            }
        }
        // The chunk and its line end.
        let want = size + 2;
        let sent = io::copy(&mut Read::by_ref(from).take(want), to)?;
        if sent < want {
            return Ok(false);
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
    fn what_is_no_answer_is_not_carried_and_a_header_in_it_does_not_reach_the_browser() {
        let big = format!(
            "HTTP/1.1 200 OK\r\n{}X-Bateri-Net-Error: -102\r\n\r\n",
            "X-Pad: 0123456789\r\n".repeat(10_000)
        );
        for odd in [
            // No head at all, or not an answer's first line.
            &b"garbage that is no head at all\r\nX-Bateri-Net-Error: -102\r\n\r\n"[..],
            b"\r\nHTTP/1.1 502 Bad Gateway\r\nX-Bateri-Net-Error: -102\r\n\r\n",
            // A status line the engine could read as well as not: nothing is carried.
            b"HTTP/1.1 nine hundred\r\nX-Bateri-Net-Error: -102\r\n\r\nbody",
            b"HTTP/1.1\r\nX-Bateri-Net-Error: -102\r\n\r\n",
            // A head cut short, and nothing.
            b"HTTP/1.1 200 OK\r\nX-Bateri-Net-Error: -102\r\nContent-Le",
            b"",
            big.as_bytes(),
        ] {
            assert_eq!(
                through(odd, &[false]),
                b"",
                "{}",
                text(&odd[..odd.len().min(80)])
            );
        }
    }

    #[test]
    fn a_status_the_engine_reads_with_odd_blanks_is_read_so_and_its_header_goes() {
        for status in [
            "HTTP/1.1  502  Bad Gateway",
            "HTTP/1.1\t502",
            "HTTP/1.0 502",
            "HTTP/1.1 502Bad",
        ] {
            let answer =
                format!("{status}\r\nX-Bateri-Net-Error: -102\r\nContent-Length: 2\r\n\r\nok");
            assert_eq!(
                text(&through(answer.as_bytes(), &[false])),
                format!("{status}\r\nContent-Length: 2\r\n\r\nok")
            );
        }
    }

    #[test]
    fn what_follows_a_body_that_is_no_head_ends_the_connection_and_what_is_chunked_wrong_too() {
        // A stray line end after a body (a server's bug) ends the carrying: no head is read out
        // of what is not one, so that none passes with a header the proxy could not look for.
        let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok\r\n\
            HTTP/1.1 502 Bad Gateway\r\nX-Bateri-Net-Error: -102\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(
            text(&through(answer, &[false, false])),
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"
        );
        // A chunk size that is none: the head and what came before it, and no more.
        let answer = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\nzz\r\n\
            HTTP/1.1 502 Bad Gateway\r\nX-Bateri-Net-Error: -102\r\n\r\n";
        assert_eq!(
            text(&through(answer, &[false])),
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n"
        );
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
