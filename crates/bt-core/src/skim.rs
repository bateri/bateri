//! Skimming a read of the PTY to the next byte that matters.
//!
//! Every read of every pane is walked by more than the parser before it gets
//! there — the scanner looking for the shell's marks ([`crate::shell`]), the
//! tail following where the parser stands ([`crate::snapshot`]), the question
//! of whether a graphics command may start ([`crate::graphics`]) — and most of
//! what each walks past is bytes it has nothing to do with: text in the
//! ground state, the body of a sequence it does not read. They skim it here,
//! a block at a time, rather than each stepping through it a byte at a time.

/// How many bytes a block holds: one comparison of the block is what the
/// compiler turns into a few vector instructions.
const BLOCK: usize = 16;

/// How many bytes at the start are looked at one at a time before the blocks.
const NEAR: usize = 8;

/// How many bytes at the start of `bytes` `stop` says nothing about: the
/// index of the first byte it stops at, or the length when none.
///
/// `stop` is asked of every byte of a block before the block is judged —
/// with no early exit inside a block the compiler compares the block at once —
/// and then of the block's bytes one at a time to find which.
#[inline]
pub(crate) fn until(bytes: &[u8], stop: impl Fn(u8) -> bool) -> usize {
    // A stop within the first bytes is the common one in a stream thick with
    // sequences, and a block's comparison and then the walk of the block for
    // the byte cost more than the bytes themselves.
    let (near, far) = bytes.split_at(bytes.len().min(NEAR));
    if let Some(at) = near.iter().position(|&byte| stop(byte)) {
        return at;
    }
    let mut at = 0;
    for block in far.chunks_exact(BLOCK) {
        if block.iter().fold(false, |any, &byte| any | stop(byte)) {
            break;
        }
        at += BLOCK;
    }
    let rest = far.get(at..).unwrap_or_default();
    near.len()
        + at
        + rest
            .iter()
            .position(|&byte| stop(byte))
            .unwrap_or(rest.len())
}

/// The `ESC` bytes of one read, found once for everything that walks the read.
///
/// The scanner, the tail and the question of whether a graphics command may start
/// each want to know where the escape sequences are, and in a stream of colored
/// text that is one every few bytes: three searches for the same bytes, and three
/// walks over the same parameters of the same CSI. The search is made here and
/// they read the answer. A CSI that is whole in the read (`ESC [`, parameters and
/// intermediates, a final byte) is recorded with its length, so none of them walks
/// it again; any other `ESC` is recorded as the place it is.
///
/// Positions are the read's own. An index serves one read at a time ([`Escapes::find`]
/// starts it afresh) and keeps its memory from one to the next.
#[derive(Debug, Default)]
pub(crate) struct Escapes {
    found: Vec<Esc>,
}

/// One `ESC` of a read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Esc {
    /// Where the `ESC` is.
    pub(crate) at: usize,
    /// How long the CSI that starts here is, `ESC` included, when it is whole in the read
    /// and plain: parameters and intermediates (0x20..=0x3f) and a final byte
    /// (0x40..=0x7e), nothing else. Zero for any other `ESC`.
    pub(crate) csi: usize,
}

/// How many entries of the index a pane keeps once a read is done: a read of nothing
/// but `ESC` must not pin its size for the session's life.
const ESCAPES_KEEP: usize = 4096;

impl Escapes {
    /// The index of `bytes`, for a caller that asks about a single read.
    #[cfg(test)]
    pub(crate) fn of(bytes: &[u8]) -> Self {
        let mut escapes = Self::default();
        escapes.find(bytes);
        escapes
    }

    /// Finds the `ESC`s of `bytes`, which replace those of the read before.
    pub(crate) fn find(&mut self, bytes: &[u8]) {
        self.found.clear();
        let mut at = 0;
        while let Some(rest) = bytes.get(at..) {
            let esc = at + until(rest, |byte| byte == 0x1b);
            if esc >= bytes.len() {
                break;
            }
            let csi = plain_csi(bytes.get(esc..).unwrap_or_default());
            self.found.push(Esc { at: esc, csi });
            // A CSI holds no `ESC`; after any other the next byte may be one.
            at = esc + csi.max(1);
        }
    }

    /// Lets go of the memory a very thick read grew.
    pub(crate) fn release(&mut self) {
        if self.found.capacity() > ESCAPES_KEEP {
            self.found.clear();
            self.found.shrink_to(ESCAPES_KEEP);
        }
    }

    /// The first `ESC` at `from` or after it, given the place `cursor` of the
    /// last look: a walker goes forward through a read and gives the cursor back
    /// to the next look, so no look starts at the beginning.
    pub(crate) fn next(&self, cursor: &mut usize, from: usize) -> Option<Esc> {
        while let Some(esc) = self.found.get(*cursor) {
            if esc.at >= from {
                return Some(*esc);
            }
            *cursor += 1;
        }
        None
    }

    /// Whether a read of the PTY may begin an application program command: it has
    /// `ESC _` in it, or ends in the `ESC` that may be its first half. `bytes` is
    /// the read this index was made of.
    pub(crate) fn may_begin_command(&self, bytes: &[u8]) -> bool {
        self.found
            .iter()
            .any(|esc| esc.csi == 0 && matches!(bytes.get(esc.at + 1), None | Some(b'_')))
    }
}

/// The length of the plain CSI `bytes` starts with, when all of it is there, else 0.
/// `bytes` starts at an `ESC`.
fn plain_csi(bytes: &[u8]) -> usize {
    let Some(body) = bytes.strip_prefix(b"\x1b[") else {
        return 0;
    };
    let params = until(body, |byte| !matches!(byte, 0x20..=0x3f));
    if matches!(body.get(params), Some(0x40..=0x7e)) {
        2 + params + 1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a read may begin, as it was asked by walking to each `ESC` in turn,
    /// before the index was made once for all.
    fn may_begin_command_by_walking(bytes: &[u8]) -> bool {
        let mut rest = bytes;
        loop {
            let at = rest
                .iter()
                .position(|&byte| byte == 0x1b)
                .unwrap_or(rest.len());
            match rest.get(at..) {
                None | Some([]) => return false,
                Some([_]) | Some([_, b'_', ..]) => return true,
                Some([_, after @ ..]) => rest = after,
            }
        }
    }

    /// The index holds every `ESC` of a read and says of each how long the plain CSI that
    /// starts there is; and a read may begin a command where walking from `ESC` to `ESC`
    /// says so — on text, on streams thick with sequences, and on the bytes that make and
    /// unmake a pair or a CSI.
    #[test]
    fn the_index_of_a_read_says_what_walking_it_says() {
        let alphabet: &[u8] = b"\x1b\x1b\x1b[[[_ _a;m0\\\x07\xc3 J?";
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            usize::try_from(seed % bound as u64).unwrap_or(0)
        };
        let mut reads: Vec<Vec<u8>> = Vec::new();
        for len in 0..70 {
            for _ in 0..60 {
                reads.push(
                    (0..len)
                        .map(|_| alphabet.get(next(alphabet.len())).copied().unwrap_or(b'x'))
                        .collect(),
                );
            }
            // Text with the pair, an `ESC` alone and a CSI put in at each place.
            for at in 0..len {
                for put in [&b"\x1b"[..], b"\x1b_", b"\x1b[31m", b"\x1b[3"] {
                    let mut bytes = vec![b'a'; len];
                    for (offset, &byte) in put.iter().enumerate() {
                        if let Some(slot) = bytes.get_mut(at + offset) {
                            *slot = byte;
                        }
                    }
                    reads.push(bytes);
                }
            }
        }
        let mut reused = Escapes::default();
        for bytes in &reads {
            let escapes = Escapes::of(bytes);
            reused.find(bytes);
            assert_eq!(
                escapes.found, reused.found,
                "{bytes:?}: one index serves reads in turn"
            );
            let places: Vec<usize> = bytes
                .iter()
                .enumerate()
                .filter(|&(_, &byte)| byte == 0x1b)
                .map(|(at, _)| at)
                .collect();
            let found: Vec<usize> = escapes.found.iter().map(|esc| esc.at).collect();
            assert_eq!(found, places, "{bytes:?}");
            for esc in &escapes.found {
                // A CSI as the step reads it: parameters, then a final byte, whole.
                let tail = bytes.get(esc.at + 1..).unwrap_or_default();
                let whole = if tail.first() == Some(&b'[') {
                    tail.iter()
                        .skip(1)
                        .position(|byte| !matches!(byte, 0x20..=0x3f))
                        .filter(|&params| matches!(tail.get(1 + params), Some(0x40..=0x7e)))
                        .map(|params| 2 + params + 1)
                } else {
                    None
                };
                assert_eq!(esc.csi, whole.unwrap_or(0), "{bytes:?} at {}", esc.at);
            }
            assert_eq!(
                escapes.may_begin_command(bytes),
                may_begin_command_by_walking(bytes),
                "{bytes:?}"
            );
        }
    }

    /// The cursor goes forward through the index and never back, whatever the walker
    /// asks it from.
    #[test]
    fn the_cursor_finds_the_next_esc_from_wherever_the_walker_is() {
        let bytes = b"a\x1b[mb\x1b]0;\x07\x1b";
        let escapes = Escapes::of(bytes);
        let mut cursor = 0;
        assert_eq!(
            escapes.next(&mut cursor, 0).map(|esc| (esc.at, esc.csi)),
            Some((1, 3))
        );
        assert_eq!(
            escapes.next(&mut cursor, 2).map(|esc| (esc.at, esc.csi)),
            Some((5, 0))
        );
        assert_eq!(
            escapes.next(&mut cursor, 6).map(|esc| (esc.at, esc.csi)),
            Some((10, 0))
        );
        assert_eq!(escapes.next(&mut cursor, 11), None);
    }

    /// The block walk and the byte walk find the same byte, wherever it is:
    /// in the first block, on a block's edge, in the remainder, nowhere.
    #[test]
    fn the_first_stop_is_found_wherever_it_is() {
        for len in 0..70 {
            for stop_at in 0..=len {
                let mut bytes = vec![b'a'; len];
                if let Some(byte) = bytes.get_mut(stop_at) {
                    *byte = 0x1b;
                }
                // A second stop behind the first is not the one found.
                if let Some(byte) = bytes.get_mut(stop_at + 3) {
                    *byte = 0x1b;
                }
                assert_eq!(
                    until(&bytes, |byte| byte == 0x1b),
                    stop_at,
                    "{len} {stop_at}"
                );
            }
        }
    }
}
