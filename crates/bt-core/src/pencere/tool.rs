//! The name of the tool an `open` asks to call, read from the message's JSON.
//!
//! The crate that holds the protocol's rules reads an `open` whole and does not
//! hand the tool's name out until the view starts, which is after the question
//! to the person ([`Action::Consent`](::pencere::session::Action::Consent)) has
//! been asked and answered. The shell remembers an answer by the tool it was for,
//! so it needs the name at the question, and this is the one member it needs:
//! `tool.name` of the object the program sent.
//!
//! It is a reading and not a judgment — the crate still decides whether the open
//! is well-formed, whether its token is right and whether a view may start — so
//! anything it does not understand is `None`, and the shell then does not
//! remember (it asks again), which is the safe way to be wrong. The text is the
//! program's, so the reader is bounded: it skips what it does not want without
//! building it, keeps no more than [`NAME_LIMIT`] bytes and goes no deeper than
//! [`DEPTH`] levels.

/// The longest tool name read, in bytes: one that is longer is not read.
const NAME_LIMIT: usize = 256;

/// How deeply nested a value that is skipped may be.
const DEPTH: usize = 32;

/// The tool an `open`'s payload names: `{"tool":{"name":"…"}}`, wherever the
/// object puts it among its members. `None` when the text is not that.
pub(super) fn open_tool(payload: &str) -> Option<String> {
    let mut scan = Scan {
        bytes: payload.as_bytes(),
        at: 0,
    };
    scan.member("tool", DEPTH, |scan, depth| {
        scan.member("name", depth, |scan, _| scan.string(NAME_LIMIT))
    })
    .filter(|name| !name.is_empty())
}

struct Scan<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Scan<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        self.space();
        (self.peek() == Some(byte)).then(|| self.at += 1)
    }

    /// Walks the object here and, at the first member named `wanted`, reads
    /// its value with `read`; the other members are skipped. `None` when the
    /// value is not an object or has no such member.
    fn member<T>(
        &mut self,
        wanted: &str,
        depth: usize,
        read: impl FnOnce(&mut Self, usize) -> Option<T>,
    ) -> Option<T> {
        let depth = depth.checked_sub(1)?;
        self.eat(b'{')?;
        self.space();
        if self.peek() == Some(b'}') {
            return None;
        }
        loop {
            // A key too long to read ends the reading: the object is not one
            // that is understood, and the name is not guessed at.
            let key = self.string(NAME_LIMIT)?;
            self.eat(b':')?;
            if key == wanted {
                return read(self, depth);
            }
            self.skip(depth)?;
            self.space();
            match self.peek()? {
                b',' => self.at += 1,
                _ => return None,
            }
            self.space();
        }
    }

    /// Reads a JSON string, up to `limit` bytes of it: `None` past that, or
    /// if it is not one. The escapes JSON has are read; a lone surrogate is
    /// the replacement character.
    fn string(&mut self, limit: usize) -> Option<String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let byte = self.peek()?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                0..=0x1f => return None,
                b'\\' => {
                    let escaped = self.peek()?;
                    self.at += 1;
                    let ch = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => return None,
                    };
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                }
                other => out.push(other),
            }
            if out.len() > limit {
                return None;
            }
        }
    }

    /// The four hex digits after `\u`, and the second escape that completes a
    /// surrogate pair.
    fn unicode(&mut self) -> Option<char> {
        let first = self.hex4()?;
        if !(0xd800..0xdc00).contains(&first) {
            return Some(char::from_u32(first).unwrap_or('\u{fffd}'));
        }
        // A high surrogate wants a low one right behind it.
        if self
            .bytes
            .get(self.at..)
            .is_some_and(|rest| rest.starts_with(b"\\u"))
        {
            let saved = self.at;
            self.at += 2;
            let second = self.hex4()?;
            if (0xdc00..0xe000).contains(&second) {
                let code = 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00);
                return Some(char::from_u32(code).unwrap_or('\u{fffd}'));
            }
            self.at = saved;
        }
        Some('\u{fffd}')
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.bytes.get(self.at..self.at + 4)?;
        let mut value = 0u32;
        for digit in digits {
            value = value * 16 + char::from(*digit).to_digit(16)?;
        }
        self.at += 4;
        Some(value)
    }

    /// Steps over one value without keeping it.
    fn skip(&mut self, depth: usize) -> Option<()> {
        self.space();
        match self.peek()? {
            b'"' => self.skip_string(),
            open @ (b'{' | b'[') => {
                let depth = depth.checked_sub(1)?;
                self.at += 1;
                let close = if open == b'{' { b'}' } else { b']' };
                self.space();
                if self.peek() == Some(close) {
                    self.at += 1;
                    return Some(());
                }
                loop {
                    if open == b'{' {
                        self.skip_string()?;
                        self.eat(b':')?;
                    }
                    self.skip(depth)?;
                    self.space();
                    match self.peek()? {
                        b',' => self.at += 1,
                        byte if byte == close => {
                            self.at += 1;
                            return Some(());
                        }
                        _ => return None,
                    }
                }
            }
            _ => {
                // A number, `true`, `false` or `null`: to the next delimiter.
                let start = self.at;
                while !matches!(
                    self.peek(),
                    None | Some(b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n')
                ) {
                    self.at += 1;
                }
                (self.at > start).then_some(())
            }
        }
    }

    fn skip_string(&mut self) -> Option<()> {
        self.space();
        if self.peek() != Some(b'"') {
            return None;
        }
        self.at += 1;
        loop {
            match self.peek()? {
                b'"' => {
                    self.at += 1;
                    return Some(());
                }
                b'\\' => self.at += 2,
                0..=0x1f => return None,
                _ => self.at += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_is_read_wherever_the_object_keeps_it() {
        for payload in [
            r#"{"tool":{"name":"list"}}"#,
            r#"{"token":"abc","tool":{"arguments":{"name":"decoy"},"name":"list"}}"#,
            r#" { "bundle" : { "meta" : { "tool" : { "name" : "decoy" } } , "files":[1,2.5e3,true,null,"x"] } , "tool" : { "name" : "list" , "arguments" : { } } } "#,
        ] {
            assert_eq!(open_tool(payload).as_deref(), Some("list"), "{payload}");
        }
    }

    #[test]
    fn a_name_is_read_with_its_escapes() {
        assert_eq!(
            open_tool(r#"{"tool":{"name":"a\"b\\c\/dA😀"}}"#).as_deref(),
            Some("a\"b\\c/dA\u{1f600}")
        );
        assert_eq!(
            open_tool(r#"{"tool":{"name":"\ud83d x"}}"#).as_deref(),
            Some("\u{fffd} x"),
            "a lone surrogate is the replacement character"
        );
    }

    #[test]
    fn what_is_not_an_open_names_no_tool() {
        for payload in [
            "",
            "[]",
            "{}",
            r#"{"tool":"list"}"#,
            r#"{"tool":{}}"#,
            r#"{"tool":{"name":""}}"#,
            r#"{"tool":{"name":5}}"#,
            r#"{"tool":{"name":"unterminated}}"#,
            r#"{"tool":{"name":"bad \x escape"}}"#,
            "{\"tool\":{\"name\":\"control \u{1}\"}}",
            r#"{"other":1}"#,
            r#"{"tool" {"name":"x"}}"#,
        ] {
            assert_eq!(open_tool(payload), None, "{payload:?}");
        }
    }

    #[test]
    fn a_name_past_the_limit_is_not_read() {
        let long = "x".repeat(NAME_LIMIT + 1);
        assert_eq!(
            open_tool(&format!(r#"{{"tool":{{"name":"{long}"}}}}"#)),
            None
        );
        let fits = "x".repeat(NAME_LIMIT);
        assert_eq!(
            open_tool(&format!(r#"{{"tool":{{"name":"{fits}"}}}}"#)).as_deref(),
            Some(fits.as_str())
        );
    }

    #[test]
    fn nesting_past_the_depth_is_not_followed() {
        let deep = format!("{}1{}", "[".repeat(DEPTH + 8), "]".repeat(DEPTH + 8));
        assert_eq!(
            open_tool(&format!(r#"{{"a":{deep},"tool":{{"name":"x"}}}}"#)),
            None
        );
        let shallow = format!("{}1{}", "[".repeat(8), "]".repeat(8));
        assert_eq!(
            open_tool(&format!(r#"{{"a":{shallow},"tool":{{"name":"x"}}}}"#)).as_deref(),
            Some("x")
        );
    }
}
