//! What a program's web page needs to know of a font file to declare it: whether
//! the face is italic and which weights it covers.
//!
//! The grid asks the font system for a face by name and gets the right file; the
//! web engine of a program's view is given the files and has to be told which one
//! is which (`@font-face`'s `font-style` and `font-weight`). Both answers are the
//! file's own — the `OS/2` and `head` tables say if it is italic and what weight
//! class it is, and a variable font's `fvar` table gives the span of its `wght`
//! axis — so they are read from the file, not written in a list beside it that a
//! new font would have to remember.
//!
//! Only the table directory and three small tables are read, with seeks: a font of
//! a megabyte is not read to learn two facts. Platformless, so it is tested on the
//! shipped files on any machine.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The most tables a directory may name before the file is not taken for a font.
const MAX_TABLES: usize = 256;

/// The most bytes of the `fvar` table read: its header and the axes.
const MAX_FVAR: usize = 4096;

/// The weights CSS knows: a face outside the range is not one.
const WEIGHTS: std::ops::RangeInclusive<i32> = 1..=1000;

/// A face of a font file, as the page declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Style {
    /// Italic or oblique (the page's `font-style: italic`).
    pub italic: bool,
    /// The weights it covers, lowest and highest on the CSS scale: one value twice for a
    /// static font, the span of the `wght` axis for a variable one.
    pub weight: (u16, u16),
}

/// The style of the font file at `path`; `None` when it is not a single font (a
/// collection, a file with no tables, one that cannot be read).
pub(crate) fn style_of(path: &Path) -> Option<Style> {
    read_style(&mut File::open(path).ok()?)
}

/// [`style_of`] on anything that can be read and sought in.
pub(crate) fn read_style(file: &mut (impl Read + Seek)) -> Option<Style> {
    let header = bytes_at(file, 0, 12)?;
    // TrueType (`0x00010000`, `true`) or CFF (`OTTO`). A collection (`ttcf`) holds more than
    // one face and none of the app's files is one.
    let single = matches!(header.get(..4)?, [0, 1, 0, 0] | b"true" | b"OTTO");
    if !single {
        return None;
    }
    let count = usize::from(u16_at(&header, 4)?);
    if count == 0 || count > MAX_TABLES {
        return None;
    }
    let directory = bytes_at(file, 12, count * 16)?;
    let table = |tag: &[u8; 4]| -> Option<(u64, usize)> {
        directory.chunks_exact(16).find_map(|entry| {
            (entry.get(..4)? == tag).then(|| {
                let offset = u32_at(entry, 8)?;
                let length = u32_at(entry, 12)?;
                Some((u64::from(offset), usize::try_from(length).ok()?))
            })?
        })
    };

    // `head`'s `macStyle`: bit 0 bold, bit 1 italic.
    let mac = table(b"head")
        .filter(|&(_, length)| length >= 46)
        .and_then(|(offset, _)| u16_at(&bytes_at(file, offset, 46)?, 44))
        .unwrap_or(0);
    // `OS/2`'s weight class and `fsSelection` (bit 0 italic, bit 9 oblique).
    let os2 = table(b"OS/2")
        .filter(|&(_, length)| length >= 64)
        .and_then(|(offset, _)| bytes_at(file, offset, 64));
    let class = os2.as_deref().and_then(|os2| u16_at(os2, 4));
    let selection = os2.as_deref().and_then(|os2| u16_at(os2, 62)).unwrap_or(0);

    let italic = selection & (1 << 0) != 0 || selection & (1 << 9) != 0 || mac & (1 << 1) != 0;
    let fixed = class
        .map(i32::from)
        .unwrap_or(if mac & 1 != 0 { 700 } else { 400 });
    let (low, high) = table(b"fvar")
        .and_then(|(offset, length)| {
            let fvar = bytes_at(file, offset, length.min(MAX_FVAR))?;
            weight_axis(&fvar)
        })
        .unwrap_or((fixed, fixed));
    let clamp = |weight: i32| u16::try_from(weight.clamp(*WEIGHTS.start(), *WEIGHTS.end())).ok();
    Some(Style {
        italic,
        weight: (clamp(low)?, clamp(high)?),
    })
}

/// The lowest and highest value of the `wght` axis of an `fvar` table, in whole units.
fn weight_axis(fvar: &[u8]) -> Option<(i32, i32)> {
    let first = usize::from(u16_at(fvar, 4)?);
    let count = usize::from(u16_at(fvar, 8)?);
    let size = usize::from(u16_at(fvar, 10)?);
    // An axis record: tag, minimum, default, maximum (16.16 fixed), flags, name.
    if size < 16 {
        return None;
    }
    (0..count).find_map(|axis| {
        let at = first.checked_add(axis.checked_mul(size)?)?;
        let record = fvar.get(at..at.checked_add(16)?)?;
        if record.get(..4)? != b"wght" {
            return None;
        }
        // 16.16 fixed point: the lowest value rounds down and the highest up, so the face
        // is declared for every weight the axis reaches.
        let fixed = |at: usize| {
            Some(i32::from_be_bytes(
                record.get(at..at.checked_add(4)?)?.try_into().ok()?,
            ))
        };
        let (low, high) = (fixed(4)? >> 16, fixed(12)?.saturating_add(0xFFFF) >> 16);
        (low <= high).then_some((low, high))
    })
}

fn bytes_at(file: &mut (impl Read + Seek), offset: u64, length: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

/// A big-endian `u16` at `at`, `None` when it lies outside `bytes`. The one reader of the
/// crate's font tables: [`crate::features`] reads its tables with it too.
pub(crate) fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let pair = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([*pair.first()?, *pair.get(1)?]))
}

/// A big-endian `u32` at `at`, `None` when it lies outside `bytes`.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let four = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes(four.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;

    use super::*;

    /// A font file of the tables given, each as (tag, body).
    fn font(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut file = vec![0, 1, 0, 0];
        file.extend((tables.len() as u16).to_be_bytes());
        file.extend([0; 6]);
        let mut offset = 12 + tables.len() * 16;
        let mut bodies: Vec<u8> = Vec::new();
        for (tag, body) in tables {
            file.extend(tag.iter());
            file.extend([0; 4]);
            file.extend((offset as u32).to_be_bytes());
            file.extend((body.len() as u32).to_be_bytes());
            offset += body.len();
            bodies.extend(body.iter());
        }
        file.extend(bodies);
        file
    }

    fn os2(class: u16, selection: u16) -> Vec<u8> {
        let mut table = vec![0; 96];
        table[4..6].copy_from_slice(&class.to_be_bytes());
        table[62..64].copy_from_slice(&selection.to_be_bytes());
        table
    }

    fn head(mac: u16) -> Vec<u8> {
        let mut table = vec![0; 54];
        table[44..46].copy_from_slice(&mac.to_be_bytes());
        table
    }

    fn fvar(axes: &[(&[u8; 4], i32, i32)]) -> Vec<u8> {
        let mut table = vec![0, 1, 0, 0, 0, 16, 0, 2];
        table.extend((axes.len() as u16).to_be_bytes());
        table.extend(20u16.to_be_bytes());
        table.extend([0; 4]);
        for (tag, low, high) in axes {
            table.extend(tag.iter());
            table.extend((low << 16).to_be_bytes());
            table.extend((low << 16).to_be_bytes());
            table.extend((high << 16).to_be_bytes());
            table.extend([0; 4]);
        }
        table
    }

    fn style(bytes: Vec<u8>) -> Option<Style> {
        read_style(&mut Cursor::new(bytes))
    }

    #[test]
    fn a_static_face_takes_its_weight_class_and_its_italic_bit_from_the_os2_table() {
        let regular = style(font(&[(b"OS/2", os2(400, 0x40)), (b"head", head(0))])).unwrap();
        assert_eq!((regular.italic, regular.weight), (false, (400, 400)));
        let bold_italic = style(font(&[(b"OS/2", os2(700, 0x01)), (b"head", head(3))])).unwrap();
        assert_eq!((bold_italic.italic, bold_italic.weight), (true, (700, 700)));
    }

    #[test]
    fn an_oblique_face_or_one_that_only_the_head_table_calls_italic_is_italic() {
        let oblique = style(font(&[(b"OS/2", os2(400, 1 << 9))])).unwrap();
        assert!(oblique.italic);
        let mac = style(font(&[(b"OS/2", os2(400, 0)), (b"head", head(2))])).unwrap();
        assert!(mac.italic);
    }

    #[test]
    fn a_variable_face_covers_the_span_of_its_weight_axis() {
        let axes = fvar(&[(b"wdth", 75, 125), (b"wght", 100, 800)]);
        let variable = style(font(&[(b"OS/2", os2(400, 0x40)), (b"fvar", axes)])).unwrap();
        assert_eq!(variable.weight, (100, 800));
        // An axis that is not weight leaves the face static.
        let other = fvar(&[(b"slnt", -10, 0)]);
        let fixed = style(font(&[(b"OS/2", os2(300, 0)), (b"fvar", other)])).unwrap();
        assert_eq!(fixed.weight, (300, 300));
    }

    #[test]
    fn a_face_without_an_os2_table_is_bold_by_its_head_table_or_regular() {
        assert_eq!(
            style(font(&[(b"head", head(1))])).unwrap().weight,
            (700, 700)
        );
        assert_eq!(
            style(font(&[(b"head", head(0))])).unwrap().weight,
            (400, 400)
        );
    }

    #[test]
    fn what_is_not_one_font_has_no_style() {
        assert_eq!(style(b"ttcf\0\x01\0\x00".to_vec()), None, "a collection");
        assert_eq!(style(b"%PDF-1.7".to_vec()), None, "another format");
        assert_eq!(style(Vec::new()), None, "an empty file");
        assert_eq!(
            style(vec![0, 1, 0, 0, 0xff, 0xff, 0, 0, 0, 0, 0, 0]),
            None,
            "too many tables"
        );
        let mut cut = font(&[(b"OS/2", os2(400, 0))]);
        cut.truncate(20);
        assert_eq!(style(cut), None, "a directory that stops short");
    }

    #[test]
    fn a_fractional_weight_axis_is_declared_out_to_the_weights_it_reaches() {
        let mut axes = fvar(&[(b"wght", 100, 800)]);
        // The maximum is 799.5: 16.16 fixed, 0x031F_8000.
        let at = axes.len() - 8;
        axes[at..at + 4].copy_from_slice(&0x031F_8000u32.to_be_bytes());
        let style = style(font(&[(b"fvar", axes)])).unwrap();
        assert_eq!(style.weight, (100, 800));
    }

    #[test]
    fn a_weight_outside_the_css_scale_is_brought_in() {
        let wide = fvar(&[(b"wght", 0, 1200)]);
        let style = style(font(&[(b"fvar", wide)])).unwrap();
        assert_eq!(style.weight, (1, 1000));
    }

    fn shipped() -> Vec<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        crate::app_fonts::font_files(&dir)
    }

    /// The files the app ships, read for what the page is told. The numbers are printed so a
    /// new font's reading can be looked at; what is asserted is what any face must be.
    #[test]
    fn every_shipped_file_reads_and_says_what_its_name_says() {
        let files = shipped();
        assert!(files.len() >= 20, "{} files", files.len());
        for file in files {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            let style = style_of(&file).unwrap_or_else(|| panic!("{name} reads as one font"));
            eprintln!("{name}: italic {} weight {:?}", style.italic, style.weight);
            let (low, high) = style.weight;
            assert!(low <= high, "{name}");
            // The files that say italic in their name are italic and no other is.
            let named = name.contains("Italic");
            assert_eq!(style.italic, named, "{name}");
            if name.contains('[') || name.contains("VF") || name.starts_with("Cascadia") {
                assert!(low < high, "{name} is variable and covers a span");
                assert!(low <= 400 && 700 <= high, "{name} covers regular and bold");
            } else {
                assert_eq!(low, high, "{name} is static");
            }
            if name.contains("BoldItalic") || name.contains("Bold.") || name.contains("Bold-") {
                assert_eq!(low, 700, "{name}");
            }
            if name.contains("Regular") || name.contains("-Italic.") {
                assert_eq!(low, 400, "{name}");
            }
        }
    }
}
