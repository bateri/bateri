//! SHA-256, written here and not taken from a crate: what bateri downloads or sends somewhere is
//! checked against a list of these first (kablo's binaries before a server gets one, the browser
//! helper's archive before it is unpacked), and nothing else needs the function. It runs over a
//! stream, so a file of hundreds of megabytes is never held whole.
//!
//! `SHA256SUMS` is the list's file (`sha256sum`'s and `shasum -a 256`'s output): [`parse_sums`].

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// A SHA-256 being computed: [`Sha256::update`] with the bytes as they come, [`Sha256::finish`]
/// for the digest.
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        if self.filled > 0 {
            let take = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled < 64 {
                return;
            }
            let block = self.block;
            self.compress(&block);
            self.filled = 0;
        }
        let (whole, rest) = data.as_chunks::<64>();
        for block in whole {
            self.compress(block);
        }
        self.block[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }

    /// The digest as lower-case hex.
    pub fn finish(mut self) -> String {
        let bits = self.length.wrapping_mul(8);
        let mut tail = vec![0x80];
        while (self.filled + tail.len()) % 64 != 56 {
            tail.push(0);
        }
        tail.extend_from_slice(&bits.to_be_bytes());
        // The length counted by `update` is already the message's: the padding is not part of it.
        let length = self.length;
        self.update(&tail);
        self.length = length;
        self.state
            .iter()
            .map(|word| format!("{word:08x}"))
            .collect()
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
}

/// The SHA-256 of `data`, as lower-case hex.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut sha = Sha256::new();
    sha.update(data);
    sha.finish()
}

/// The SHA-256 of the file at `path`, read in pieces, and the file's length.
pub fn sha256_file(path: &Path) -> io::Result<(String, u64)> {
    let mut file = std::fs::File::open(path)?;
    let mut sha = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut length = 0u64;
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        sha.update(&buffer[..read]);
        length += read as u64;
    }
    Ok((sha.finish(), length))
}

/// Whether `text` is a SHA-256 as the lists write it: 64 lower-case hex digits.
pub fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `SHA256SUMS`' lines (`<sha>  <name>`, a `*` before the name for a binary file as `sha256sum -b`
/// writes it) as name → sum. A line that is not one is skipped.
pub fn parse_sums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (sum, name) = (words.next()?, words.next()?);
            (is_digest(sum) && words.next().is_none())
                .then(|| (name.trim_start_matches('*').to_owned(), sum.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_of_known_inputs() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(&vec![b'a'; 1_000_000]),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn pieces_of_any_size_make_the_same_digest() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let whole = sha256_hex(&data);
        for piece in [1, 3, 55, 56, 63, 64, 65, 127, 999] {
            let mut sha = Sha256::new();
            for chunk in data.chunks(piece) {
                sha.update(chunk);
            }
            assert_eq!(sha.finish(), whole, "pieces of {piece}");
        }
    }

    #[test]
    fn a_file_is_read_in_pieces_and_measured() {
        let dir = std::env::temp_dir().join(format!("bateri-digest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file");
        let data = vec![7u8; 600_000];
        std::fs::write(&path, &data).unwrap();
        let (sum, length) = sha256_file(&path).unwrap();
        assert_eq!(sum, sha256_hex(&data));
        assert_eq!(length, 600_000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sums_are_read_by_name() {
        let sum = "a".repeat(64);
        let text =
            format!("{sum}  a.zip\n{sum} *b.zip\nnot a line\n{sum}  c.zip extra\nXYZ  d.zip\n");
        let sums = parse_sums(&text);
        assert_eq!(sums.len(), 2);
        assert_eq!(sums["a.zip"], sum);
        assert_eq!(sums["b.zip"], sum);
    }
}
