//! The answer to a feature query: `a=q,i=…,e=<action>,<keys>`.
//!
//! A program that is told `OK` by a query does not know which of the protocol's
//! actions and keys this terminal does. The feature query asks about one
//! action and lists the keys and values the program means to use with it; the
//! answer repeats the ones that are understood — `e=` itself, to say the
//! action is, and each key with a value other than its default that this
//! terminal does what the protocol says with. A key that is missing from the
//! answer is not supported with that value.
//!
//! **A feature query does nothing.** It reads no payload, changes no image and
//! does not end a transmission in progress, so it is answered where it is
//! received and never parsed as a command (a value this terminal does not know
//! is not an error here, it is a key left out). **The table is what works**:
//! an entry is added with the code that does it, and a key that is not in it is
//! never answered, whatever the terminal might read of it.

use super::command::pairs;
use super::{Echo, Error, Reply};

/// What a key may be set to, for an action that reads it.
enum Values {
    /// Any number (a signed one for `z=`), as long as it is not the default,
    /// zero.
    Number,
    /// Any of these; the key's default is not among them, as it is never
    /// reported.
    Only(&'static [&'static str]),
}

type Table = &'static [(u8, Values)];

/// The keys of a transmission (`t`, `T`, `q`; and `f`, whose frames are
/// transmitted the same way).
const TRANSMIT: Table = &[
    (b'f', Values::Only(&["24", "100"])),
    (b't', Values::Only(&["f", "t", "s"])),
    (b'o', Values::Only(&["z"])),
    (b's', Values::Number),
    (b'v', Values::Number),
    (b'S', Values::Number),
    (b'O', Values::Number),
];

/// The keys of a placement (`T`, `p`).
const PLACE: Table = &[
    (b'p', Values::Number),
    (b'x', Values::Number),
    (b'y', Values::Number),
    (b'w', Values::Number),
    (b'h', Values::Number),
    (b'X', Values::Number),
    (b'Y', Values::Number),
    (b'c', Values::Number),
    (b'r', Values::Number),
    (b'z', Values::Number),
    (b'C', Values::Only(&["1"])),
    (b'U', Values::Only(&["1"])),
];

/// What `a=f` adds to a transmission: where the pixels go, which frame is the
/// canvas or is edited, the gap, how they are laid on it and the background.
const FRAME: Table = &[
    (b'x', Values::Number),
    (b'y', Values::Number),
    (b'c', Values::Number),
    (b'r', Values::Number),
    (b'z', Values::Number),
    (b'X', Values::Only(&["1"])),
    (b'Y', Values::Number),
];

/// The keys of `a=c`.
const COMPOSE: Table = &[
    (b'r', Values::Number),
    (b'c', Values::Number),
    (b'x', Values::Number),
    (b'y', Values::Number),
    (b'w', Values::Number),
    (b'h', Values::Number),
    (b'X', Values::Number),
    (b'Y', Values::Number),
    (b'C', Values::Only(&["1"])),
];

/// The keys of `a=a`: the state and number of loops, the frame and its gap, and
/// the frame that is on show.
const ANIMATE: Table = &[
    (b's', Values::Only(&["1", "2", "3"])),
    (b'v', Values::Number),
    (b'r', Values::Number),
    (b'z', Values::Number),
    (b'c', Values::Number),
];

/// The keys of `a=d`.
const DELETE: Table = &[
    (
        b'd',
        Values::Only(&[
            "A", "i", "I", "n", "N", "c", "C", "p", "P", "q", "Q", "r", "R", "x", "X", "y", "Y",
            "z", "Z", "f", "F",
        ]),
    ),
    (b'p', Values::Number),
    (b'x', Values::Number),
    (b'y', Values::Number),
    (b'z', Values::Number),
];

/// The tables of the keys an action reads, or `None` for an action that this
/// terminal does not do.
fn tables(action: u8) -> Option<&'static [Table]> {
    Some(match action {
        b't' | b'q' => &[TRANSMIT],
        b'T' => &[TRANSMIT, PLACE],
        b'p' => &[PLACE],
        b'f' => &[TRANSMIT, FRAME],
        b'a' => &[ANIMATE],
        b'c' => &[COMPOSE],
        b'd' => &[DELETE],
        _ => return None,
    })
}

/// Whether `value` is one the tables understand for `key`.
fn understood(tables: &[Table], key: u8, value: &[u8]) -> bool {
    tables
        .iter()
        .flat_map(|table| table.iter())
        .filter(|(named, _)| *named == key)
        .any(|(_, values)| match values {
            Values::Number => {
                let digits = value.strip_prefix(b"-").unwrap_or(value);
                !digits.is_empty()
                    && digits.iter().all(u8::is_ascii_digit)
                    && digits.iter().any(|&digit| digit != b'0')
                    && (key == b'z' || digits.len() == value.len())
            }
            Values::Only(only) => only.iter().any(|one| one.as_bytes() == value),
        })
}

/// The answer to `control` when it is a feature query (`a=q` with an `e=`),
/// whatever else it says; `None` for any other command.
pub(super) fn answer(control: &[u8]) -> Option<Reply> {
    let (mut query, mut asked) = (false, None);
    for (key, value) in pairs(control) {
        match key {
            b'a' => query = value == b"q",
            b'e' => asked = Some(value),
            _ => {}
        }
    }
    let asked = asked.filter(|_| query)?;
    // The placement id is a key of the query here, not the answer's address.
    let echo = Echo {
        placement: 0,
        ..Echo::lenient(control)
    };
    if echo.id == 0 {
        return Some(Reply::failed(
            echo,
            Error::invalid("a feature query needs an image id"),
        ));
    }
    let &[action] = asked else {
        return Some(Reply::ok(echo));
    };
    let Some(tables) = tables(action) else {
        return Some(Reply::ok(echo));
    };
    let mut keys = vec![format!("e={}", char::from(action))];
    for (key, value) in pairs(control) {
        if matches!(key, b'a' | b'i' | b'I' | b'm' | b'q' | b'e') || !understood(tables, key, value)
        {
            continue;
        }
        if let Ok(value) = std::str::from_utf8(value) {
            keys.push(format!("{}={value}", char::from(key)));
        }
    }
    Some(Reply::ok(echo).with_keys(keys))
}
