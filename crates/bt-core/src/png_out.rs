//! A picture of pixels as the PNG file anything reads: what `bateri view … shot` hands an agent.
//!
//! Written here and nowhere else: `png` is `bt-core`'s own dependency (the decoder of the
//! terminal's graphics, `make audit`), and the encoder is the same crate's.

use png::{BitDepth, ColorType, Compression, Encoder, SrgbRenderingIntent};

/// `rgba`, `width × height` pixels of four bytes each (red, green, blue, alpha; rows top down;
/// sRGB), as the bytes of a PNG file. The file says the pixels are sRGB, so the picture shows with
/// the colours the pane had.
///
/// `None` when the pixels are not the size they say, or are none: nothing is written for a
/// picture that has no room.
#[must_use]
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let wanted = u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)?;
    if width == 0 || height == 0 || u64::try_from(rgba.len()).ok()? != wanted {
        return None;
    }
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, width, height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    // A picture of a window is flat colour: the fast level is a few times quicker than the
    // default for files a few percent larger, and this runs on the main thread.
    encoder.set_compression(Compression::Fast);
    encoder.set_source_srgb(SrgbRenderingIntent::Perceptual);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>, bool) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .expect("a PNG that reads");
        let srgb = reader.info().srgb.is_some();
        let mut data = vec![0; reader.output_buffer_size().expect("a size")];
        let info = reader.next_frame(&mut data).expect("the frame");
        data.truncate(info.buffer_size());
        (info, data, srgb)
    }

    /// Decoded, the file is the pixels it was given, at the size it says, in eight-bit RGBA, and
    /// it says they are sRGB.
    #[test]
    fn a_png_decodes_to_the_pixels_it_was_made_from() {
        let (width, height) = (7u32, 5u32);
        let rgba: Vec<u8> = (0..width * height)
            .flat_map(|n| {
                let n = n as u8;
                [n.wrapping_mul(7), n.wrapping_mul(13), 255 - n, 255]
            })
            .collect();
        let bytes = encode_png(width, height, &rgba).expect("encoded");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let (info, data, srgb) = decoded(&bytes);
        assert_eq!((info.width, info.height), (width, height));
        assert_eq!(info.color_type, ColorType::Rgba);
        assert_eq!(info.bit_depth, BitDepth::Eight);
        assert_eq!(data, rgba);
        assert!(srgb, "the file does not say the pixels are sRGB");
    }

    #[test]
    fn pixels_that_are_not_the_size_they_say_or_are_none_are_no_picture() {
        assert!(encode_png(2, 2, &[0; 15]).is_none());
        assert!(encode_png(2, 2, &[0; 17]).is_none());
        assert!(encode_png(0, 2, &[]).is_none());
        assert!(encode_png(2, 0, &[]).is_none());
        assert!(encode_png(u32::MAX, u32::MAX, &[0; 4]).is_none());
        assert!(encode_png(1, 1, &[1, 2, 3, 255]).is_some());
    }

    /// A pane's picture is flat colour: the encoder shrinks it a great deal.
    #[test]
    fn a_flat_picture_is_a_small_file() {
        let rgba = [0x20, 0x24, 0x2c, 0xff].repeat(1280 * 800);
        let bytes = encode_png(1280, 800, &rgba).expect("encoded");
        assert!(bytes.len() < 100_000, "{} bytes", bytes.len());
    }
}
