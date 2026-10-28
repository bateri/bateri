//! The control data of one graphics command: `a=T,f=100,i=31,m=1`.
//!
//! Only the keys a command acts on are kept; an unknown key is skipped (the
//! protocol says so, and a program written for a newer terminal must still
//! get the part we understand). A value that cannot be read — a letter where
//! a number belongs, a number past `u32` — is an error, not a default: the
//! program is told (`EINVAL`) instead of getting something it did not ask for.

use super::{Echo, Error};

/// What the command asks for (`a=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// `t` (also the default): store the image, show nothing.
    Transmit,
    /// `T`: store the image and show it.
    TransmitAndPut,
    /// `p`: show a stored image.
    Put,
    /// `d`: delete placements and, for the upper-case forms, image data.
    Delete,
    /// `q`: answer as if the image were stored, then store nothing.
    Query,
}

/// How the pixels are laid out in the payload (`f=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    /// `24`: three bytes a pixel.
    Rgb,
    /// `32` (the default): four bytes a pixel.
    Rgba,
    /// `100`: a PNG file.
    Png,
}

impl Format {
    /// Bytes a pixel takes in the payload; `None` for PNG, whose size is
    /// not a function of the dimensions.
    pub(crate) fn bytes_per_pixel(self) -> Option<usize> {
        match self {
            Self::Rgb => Some(3),
            Self::Rgba => Some(4),
            Self::Png => None,
        }
    }
}

/// Where the payload comes from (`t=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Medium {
    /// `d` (the default): in the escape sequence itself.
    Direct,
    /// `f`: a file the payload names.
    File,
    /// `t`: a temporary file the payload names, deleted once it is read.
    Temp,
    /// `s`: a POSIX shared memory object the payload names, removed once it
    /// is read.
    Shared,
}

/// What a delete (`a=d`) matches, by the lower-case form of `d=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// `a`: every placement on screen.
    Visible,
    /// `i`: an image by its id (and a placement by `p=`).
    Id,
    /// `n`: the newest image with a number.
    Number,
    /// `r`: the images whose id is in `x..=y`.
    Range,
    /// `c`: the placements that cover the cursor's cell.
    Cursor,
    /// `p`: the placements that cover the cell `x=`, `y=` (1-based).
    Cell,
    /// `q`: the placements that cover the cell `x=`, `y=` and have the
    /// z-index `z=`.
    CellZ,
    /// `x`: the placements that cover the column `x=` (1-based).
    Column,
    /// `y`: the placements that cover the row `y=` (1-based).
    Row,
    /// `z`: the placements with the z-index `z=`.
    Z,
    /// `f`: animation frames, which are not supported: nothing matches.
    Frames,
}

/// One parsed command. Everything is the raw wire value except where a
/// field's doc says otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Command {
    pub(crate) action: Action,
    /// Whether `a=` was written ([`Command::is_chunk`]).
    pub(crate) explicit_action: bool,
    /// Whether no key was written but `a`, `q` and `m`
    /// ([`Command::is_chunk`]).
    pub(crate) chunk_keys_only: bool,
    pub(crate) echo: Echo,
    pub(crate) format: Format,
    pub(crate) medium: Medium,
    /// `o=z`: the payload is zlib-compressed.
    pub(crate) zlib: bool,
    /// `s=`, `v=`: the pixel dimensions of a raw payload.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// `S=`, `O=`: how many bytes to read from a file or shared memory
    /// object (0 is "to its end") and where in it they begin.
    pub(crate) data_size: u32,
    pub(crate) data_offset: u32,
    /// `m=1`: more chunks follow.
    pub(crate) more: bool,
    /// The scope of a delete and whether the data goes too (`d=` in upper case).
    pub(crate) scope: Scope,
    pub(crate) free: bool,
    /// `x=`, `y=`: a delete's range or cell, and a placement's source offset
    /// in pixels.
    pub(crate) x: u32,
    pub(crate) y: u32,
    /// `w=`, `h=`: a placement's source size in pixels; 0 is "to the edge".
    pub(crate) w: u32,
    pub(crate) h: u32,
    /// `X=`, `Y=`: where in its first cell the placement starts, pixels.
    pub(crate) cell_x: u32,
    pub(crate) cell_y: u32,
    /// `c=`, `r=`: the cells the placement covers; 0 is "work it out".
    pub(crate) columns: u32,
    pub(crate) rows: u32,
    /// `z=`: which layer the placement is drawn in.
    pub(crate) z: i32,
    /// `C=1`: placing leaves the cursor where it is.
    pub(crate) still: bool,
}

impl Command {
    /// Whether the command goes on with a transmission in progress rather
    /// than starting one of its own: it names no action, or it names nothing
    /// but the three keys a chunk carries — `m`, `q`, and a transmission's
    /// action, which kitty's own client writes on every chunk when it is not
    /// the default one (`kitten icat` sends `a=T,q=2,m=1` after its first
    /// chunk). A command with keys of its own (a size, an id, a format) is
    /// one of its own whatever its action.
    pub(crate) fn is_chunk(&self) -> bool {
        !self.explicit_action
            || (self.chunk_keys_only
                && matches!(
                    self.action,
                    Action::Transmit | Action::TransmitAndPut | Action::Query
                ))
    }

    /// Parses `control`; on failure the error carries the addressing keys
    /// that could still be read, so the program can be answered.
    pub(crate) fn parse(control: &[u8]) -> Result<Self, (Echo, Error)> {
        let echo = Echo::lenient(control);
        parse(control, echo).map_err(|error| (echo, error))
    }
}

fn parse(control: &[u8], echo: Echo) -> Result<Command, Error> {
    let mut command = Command {
        action: Action::Transmit,
        explicit_action: false,
        chunk_keys_only: true,
        echo,
        format: Format::Rgba,
        medium: Medium::Direct,
        zlib: false,
        width: 0,
        height: 0,
        data_size: 0,
        data_offset: 0,
        more: false,
        scope: Scope::Visible,
        free: false,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        cell_x: 0,
        cell_y: 0,
        columns: 0,
        rows: 0,
        z: 0,
        still: false,
    };
    if echo.id != 0 && echo.number != 0 {
        return Err(Error::invalid(
            "an image id and an image number were both given",
        ));
    }
    for (key, value) in pairs(control) {
        if !matches!(key, b'a' | b'q' | b'm') {
            command.chunk_keys_only = false;
        }
        match key {
            b'a' => {
                command.explicit_action = true;
                command.action = match letter(value)? {
                    b't' => Action::Transmit,
                    b'T' => Action::TransmitAndPut,
                    b'p' => Action::Put,
                    b'd' => Action::Delete,
                    b'q' => Action::Query,
                    b'f' | b'a' | b'c' => {
                        return Err(Error::invalid("animation is not supported"));
                    }
                    _ => return Err(Error::invalid("unknown action")),
                };
            }
            b'f' => {
                command.format = match number(value)? {
                    24 => Format::Rgb,
                    32 => Format::Rgba,
                    100 => Format::Png,
                    _ => return Err(Error::invalid("unknown pixel format")),
                };
            }
            b't' => {
                command.medium = match letter(value)? {
                    b'd' => Medium::Direct,
                    b'f' => Medium::File,
                    b't' => Medium::Temp,
                    b's' => Medium::Shared,
                    _ => return Err(Error::invalid("unknown transmission medium")),
                };
            }
            b'o' => match letter(value)? {
                b'z' => command.zlib = true,
                _ => return Err(Error::invalid("unknown compression")),
            },
            b's' => command.width = number(value)?,
            b'v' => command.height = number(value)?,
            b'S' => command.data_size = number(value)?,
            b'O' => command.data_offset = number(value)?,
            b'm' => {
                command.more = match number(value)? {
                    0 => false,
                    1 => true,
                    _ => return Err(Error::invalid("m must be 0 or 1")),
                };
            }
            b'd' => {
                let kind = letter(value)?;
                command.free = kind.is_ascii_uppercase();
                command.scope = match kind.to_ascii_lowercase() {
                    b'a' => Scope::Visible,
                    b'i' => Scope::Id,
                    b'n' => Scope::Number,
                    b'r' => Scope::Range,
                    b'c' => Scope::Cursor,
                    b'p' => Scope::Cell,
                    b'q' => Scope::CellZ,
                    b'x' => Scope::Column,
                    b'y' => Scope::Row,
                    b'z' => Scope::Z,
                    b'f' => Scope::Frames,
                    _ => return Err(Error::invalid("unknown delete specifier")),
                };
            }
            b'x' => command.x = number(value)?,
            b'y' => command.y = number(value)?,
            b'w' => command.w = number(value)?,
            b'h' => command.h = number(value)?,
            b'X' => command.cell_x = number(value)?,
            b'Y' => command.cell_y = number(value)?,
            b'c' => command.columns = number(value)?,
            b'r' => command.rows = number(value)?,
            b'z' => command.z = signed(value)?,
            b'C' => command.still = number(value)? == 1,
            // Two ways of placing an image that this terminal does not do:
            // refusing is better than drawing it in the wrong place.
            b'U' if number(value)? != 0 => {
                return Err(Error::invalid("unicode placeholders are not supported"));
            }
            b'P' | b'Q' if number(value)? != 0 => {
                return Err(Error::invalid("relative placement is not supported"));
            }
            // `i`, `I`, `p` and `q` are the echo's; the rest is not read.
            _ => {}
        }
    }
    Ok(command)
}

/// The `key=value` pairs; a key is one byte and a pair without `=` is
/// skipped.
pub(super) fn pairs(control: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    control.split(|&byte| byte == b',').filter_map(|pair| {
        let (key, value) = pair.split_first()?;
        let value = value.strip_prefix(b"=")?;
        Some((*key, value))
    })
}

/// Whether unreadable control data was meant as a query (`a=q`), whose
/// answer goes by another road than the stream's.
pub(super) fn is_query(control: &[u8]) -> bool {
    pairs(control).any(|(key, value)| key == b'a' && value == b"q")
}

/// A decimal `u32`: digits only, so `+1`, ` 1` and `1.0` are not numbers.
pub(super) fn number(value: &[u8]) -> Result<u32, Error> {
    if value.is_empty() || !value.iter().all(u8::is_ascii_digit) {
        return Err(Error::invalid("a number was expected"));
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| Error::invalid("a number is out of range"))
}

/// A decimal `i32`, as `z=` writes it: an optional minus sign and digits.
pub(super) fn signed(value: &[u8]) -> Result<i32, Error> {
    let digits = value.strip_prefix(b"-").unwrap_or(value);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(Error::invalid("a number was expected"));
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| Error::invalid("a number is out of range"))
}

/// A single-letter value.
fn letter(value: &[u8]) -> Result<u8, Error> {
    match value {
        [byte] if byte.is_ascii_alphabetic() => Ok(*byte),
        _ => Err(Error::invalid("a single letter was expected")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(control: &str) -> Command {
        Command::parse(control.as_bytes()).expect("parses")
    }

    fn failed(control: &str) -> (Echo, Error) {
        Command::parse(control.as_bytes()).expect_err("fails")
    }

    #[test]
    fn the_defaults_are_a_direct_rgba_transmission() {
        let command = parsed("");
        assert_eq!(command.action, Action::Transmit);
        assert!(!command.explicit_action);
        assert_eq!(command.format, Format::Rgba);
        assert_eq!(command.medium, Medium::Direct);
        assert!(!command.zlib && !command.more);
    }

    #[test]
    fn terminal_browsers_query_parses() {
        let command = parsed("i=4207,a=q,t=d,f=24,s=1,v=1");
        assert_eq!(command.action, Action::Query);
        assert_eq!(command.echo.id, 4207);
        assert_eq!((command.width, command.height), (1, 1));
        assert_eq!(command.format, Format::Rgb);
    }

    #[test]
    fn a_transmission_with_every_option() {
        let command = parsed("a=T,f=100,o=z,i=7,p=3,q=2,m=1,C=1,z=-4,c=10,r=2");
        assert_eq!(command.action, Action::TransmitAndPut);
        assert_eq!(command.format, Format::Png);
        assert!(command.zlib && command.more);
        assert_eq!((command.echo.id, command.echo.placement), (7, 3));
        assert_eq!(command.echo.quiet, 2);
    }

    #[test]
    fn a_placement_reads_its_geometry_and_its_signed_layer() {
        let command = parsed("a=p,i=3,p=2,x=4,y=5,w=6,h=7,X=8,Y=9,c=10,r=11,z=-12,C=1");
        assert_eq!(command.action, Action::Put);
        assert_eq!((command.x, command.y, command.w, command.h), (4, 5, 6, 7));
        assert_eq!((command.cell_x, command.cell_y), (8, 9));
        assert_eq!((command.columns, command.rows), (10, 11));
        assert_eq!(command.z, -12);
        assert!(command.still);
        assert_eq!(parsed("z=2147483647").z, i32::MAX);
        assert!(!parsed("a=p,C=0").still);
        for control in ["i=4,z=2147483648", "i=4,z=-", "i=4,z=+1", "i=4,z=1-"] {
            let (echo, error) = failed(control);
            assert_eq!(echo.id, 4, "{control}");
            assert_eq!(error.code(), "EINVAL", "{control}");
        }
    }

    #[test]
    fn placements_this_terminal_cannot_do_are_refused_by_name() {
        for (control, word) in [("a=p,i=1,U=1", "unicode"), ("a=T,i=1,P=2,Q=3", "relative")] {
            let (echo, error) = failed(control);
            assert_eq!(echo.id, 1);
            assert!(error.message().contains(word), "{control}");
        }
        assert_eq!(parsed("a=p,i=1,U=0,P=0,Q=0,H=4").action, Action::Put);
    }

    #[test]
    fn unknown_keys_and_pairs_without_a_value_are_skipped() {
        let command = parsed("a=t,Z=9,junk,,i=5");
        assert_eq!(command.echo.id, 5);
        assert_eq!(command.action, Action::Transmit);
    }

    #[test]
    fn a_delete_remembers_the_case_of_its_specifier() {
        let lower = parsed("a=d,d=i,i=3");
        assert_eq!((lower.scope, lower.free), (Scope::Id, false));
        let upper = parsed("a=d,d=I,i=3");
        assert_eq!((upper.scope, upper.free), (Scope::Id, true));
        let range = parsed("a=d,d=R,x=2,y=9");
        assert_eq!((range.scope, range.x, range.y), (Scope::Range, 2, 9));
        assert_eq!(parsed("a=d").scope, Scope::Visible);
        assert_eq!(parsed("a=d,d=Z").scope, Scope::Z);
        assert_eq!(parsed("a=d,d=c").scope, Scope::Cursor);
        assert_eq!(parsed("a=d,d=P").scope, Scope::Cell);
        assert_eq!(parsed("a=d,d=q").scope, Scope::CellZ);
        assert_eq!(parsed("a=d,d=X").scope, Scope::Column);
        assert_eq!(parsed("a=d,d=y").scope, Scope::Row);
        assert_eq!(parsed("a=d,d=F").scope, Scope::Frames);
    }

    #[test]
    fn what_cannot_be_read_is_an_error_that_still_names_the_image() {
        for control in [
            "i=4,a=x",
            "i=4,f=7",
            "i=4,s=-1",
            "i=4,s=99999999999",
            "i=4,t=q",
            "i=4,o=g",
            "i=4,m=2",
            "i=4,a=d,d=!",
        ] {
            let (echo, error) = failed(control);
            assert_eq!(echo.id, 4, "{control}");
            assert_eq!(error.code(), "EINVAL", "{control}");
        }
    }

    #[test]
    fn animation_is_refused_by_name() {
        for action in ["f", "a", "c"] {
            let (_, error) = failed(&format!("a={action},i=1"));
            assert_eq!(error.code(), "EINVAL");
            assert!(error.message().contains("animation"));
        }
    }

    #[test]
    fn an_id_and_a_number_together_are_refused() {
        let (echo, error) = failed("i=1,I=2");
        assert_eq!((echo.id, echo.number), (1, 2));
        assert_eq!(error.code(), "EINVAL");
    }

    #[test]
    fn the_three_indirect_media_are_told_apart() {
        for (letter, medium) in [
            ("f", Medium::File),
            ("t", Medium::Temp),
            ("s", Medium::Shared),
        ] {
            assert_eq!(parsed(&format!("t={letter}")).medium, medium);
        }
        assert_eq!(parsed("t=d").medium, Medium::Direct);
    }

    #[test]
    fn a_file_read_has_a_size_and_an_offset() {
        let command = parsed("t=f,S=300,O=16");
        assert_eq!((command.data_size, command.data_offset), (300, 16));
        let command = parsed("t=f");
        assert_eq!((command.data_size, command.data_offset), (0, 0));
        let (_, error) = failed("i=4,S=-1");
        assert_eq!(error.code(), "EINVAL");
    }
}
