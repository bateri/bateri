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

/// How many bytes at the start of `bytes` `stop` says nothing about: the
/// index of the first byte it stops at, or the length when none.
///
/// `stop` is asked of every byte of a block before the block is judged —
/// with no early exit inside a block the compiler compares the block at once —
/// and then of the block's bytes one at a time to find which.
#[inline]
pub(crate) fn until(bytes: &[u8], stop: impl Fn(u8) -> bool) -> usize {
    let mut at = 0;
    for block in bytes.chunks_exact(BLOCK) {
        if block.iter().fold(false, |any, &byte| any | stop(byte)) {
            break;
        }
        at += BLOCK;
    }
    let rest = bytes.get(at..).unwrap_or_default();
    at + rest
        .iter()
        .position(|&byte| stop(byte))
        .unwrap_or(rest.len())
}

#[cfg(test)]
mod tests {
    use super::*;

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
