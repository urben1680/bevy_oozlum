//! This module contains a way to write ([`push_offset`]) and read ([`OffsetIter`]) bytes that
//! encode how many frames in the past or the future [`UpdateLog`](super::UpdateLog) as updated
//! at.
//!
//! This is more compact than storing `u64`.
//!
//! - Offsets from `0` to `127` are encoded in a single byte as `x` bits in the pattern of
//!   `0b0_xxxxxxx`.
//! - Up to `65` sequential offsets of `0` are encoded in a single byte as `x` bits in the pattern
//!   of `0b10_xxxxxx`. The numeric value of the `x` is actually read plus 2. This is because:
//!   - There is no concept of "zero times an offset of `0`" so `0b10_000000` makes no sense to be
//!     interpreted as "zero times".
//!   - The value of "one time an offset of `0`" is already encoded in `0b0_0000000`.
//! - Offsets larger than `127` are encoded in multiple bytes and are split in chunks of `x` bits:
//!   - The first and last byte of this sequence use the pattern `0b11_xxxxxx`.
//!   - If more bits are needed, in between are bytes that use the pattern `0b0_xxxxxxx`.
//!   - This uses up to ten bytes in total for `u64::MAX`.
//! - This encoding does not consume more bytes than `u64` for offsets below `2^55`.
//! - These bytes or sequences of bytes can be read in reverse as well, which is needed for reading
//!   the previous offset in [`UpdateLog::backward_log`](super::UpdateLog::backward_log)
//!   ([`many`](super::UpdateLog::backward_log_many)).
//! - The [`OffsetIter`] iterator is used to read the offsets. See [`IterItem`].

use core::{fmt::Debug, num::NonZeroU8, ops::ControlFlow};
use std::collections::{VecDeque, vec_deque::Iter};

const MAX_ZEROES_PER_BYTE: u8 = 65;
const MAX_ZEROES_AS_BYTE: u8 = 0b10_111111;
const ZEROES_MASK: u8 = 0b00_111111;
const ZEROES_OR: u8 = 0b10_000000;
const MAX_SINGLE_BYTE_OFFSET: u8 = 0b0_1111111;
const WRAPPED_OFFSET_MASK: u8 = 0b0_1111111;
const WRAPPING_OFFSET_MASK: u8 = 0b00_111111;
const WRAPPING_OFFSET_OR: u8 = 0b11_000000;
const MAX_WRAPPING_OFFSET: u8 = 0b00_111111;

pub(super) fn push_offset(
    offset_bytes: &mut VecDeque<u8>,
    index: &mut usize,
    zeroes: &mut u8,
    zeroes_max: &mut u8,
    mut offset: u64,
) {
    if offset == 0 {
        return push_zero_offset(offset_bytes, index, zeroes, zeroes_max);
    }

    if *zeroes == 1 {
        // there was an offset of 0 previously, push it now that it is sure no more such offsets
        // are following it
        offset_bytes.push_back(0);
        *index += 1;
    } else if *zeroes > 1 {
        // there was a sequence of offsets of 0 previously, push it now that it is sure no more
        // such offsets are following it
        offset_bytes.push_back((*zeroes - 2) | ZEROES_OR);
        *index += 1;
    }

    *index += 1;
    *zeroes = 0;
    *zeroes_max = 0;

    if offset <= MAX_SINGLE_BYTE_OFFSET as u64 {
        offset_bytes.push_back(offset as u8);
        return;
    }

    // this is a multi-byte offset

    let wrapping_byte = (offset & WRAPPING_OFFSET_MASK as u64) as u8 | WRAPPING_OFFSET_OR;
    offset_bytes.push_back(wrapping_byte);

    // wrapping bytes contain 6 usable bits for the offset
    offset >>= 6;

    loop {
        *index += 1;

        if offset <= MAX_WRAPPING_OFFSET as u64 {
            // this is a wrapping byte

            offset_bytes.push_back(offset as u8 | WRAPPING_OFFSET_OR);
            return;
        }

        // this is a wrapped byte

        offset_bytes.push_back((offset & WRAPPED_OFFSET_MASK as u64) as u8);

        // wrapped bytes contain 7 usable bits for the offset
        offset >>= 7;
    }
}

pub(super) fn push_zero_offset(
    offset_bytes: &mut VecDeque<u8>,
    index: &mut usize,
    zeroes: &mut u8,
    zeroes_max: &mut u8,
) {
    if *zeroes == MAX_ZEROES_PER_BYTE {
        offset_bytes.push_back(MAX_ZEROES_AS_BYTE);
        *index += 1;
        *zeroes = 0;
    }

    // increase the sequence of zero offsets
    *zeroes += 1;
    *zeroes_max = *zeroes;
}

/// Iterator to read [`UpdateLog::offset_bytes`](super::UpdateLog::offset_bytes) and decode them
/// to [`IterItem`].
#[derive(Clone)]
pub(super) struct OffsetIter<'a>(pub(super) Iter<'a, u8>);

/// Reads a byte or sequence of bytes from
/// [`UpdateLog::offset_bytes`](super::UpdateLog::offset_bytes). Contains a single offset or, if
/// the offset is `0`, a sequence of such offsets.
#[derive(Debug, PartialEq, Clone)]
pub(super) struct IterItem {
    /// Amount of frames between two updates of [`UpdateLog`](super::UpdateLog).
    pub(super) offset: u64,

    /// The amount of bytes this offset is made of to update
    /// [`UpdateLog::index`](super::UpdateLog::index) correctly. If [Self::offset] == `0`, this is
    /// the amount of `0` offsets in this byte instead.
    pub(super) len: NonZeroU8,
}

impl Debug for OffsetIter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.clone().flat_map(|IterItem { offset, len }| {
                let count = if offset == 0 { len.get() } else { 1 };
                core::iter::repeat_n(offset, count as usize)
            }))
            .finish()
    }
}

/// Decode first byte that is read by [`OffsetIter`], may be the last byte in the sequence if the
/// iterator goes backwards. Returns [`ControlFlow::Break`] if the offset consists of only one byte.
/// Otherwise returns [`ControlFlow::Continue`] with the first bits of the offset that the iterator
/// has to complete.
fn check_first_byte(byte: u8) -> ControlFlow<IterItem, u64> {
    match byte.leading_ones() {
        // 0b0_xxxxxxx => a single-byte offset
        0 => ControlFlow::Break(IterItem {
            offset: byte as u64,
            len: NonZeroU8::MIN,
        }),
        // 0b10_xxxxxx => sequence of offsets of 0 in a single byte
        1 => {
            // up to 65 zeroes, composed of 0b00_111111 = 63 ...
            // ... + 1 because it is always at least one zero
            // ... + 1 because above match arm already decodes 0b0_0000000 to len 1
            let zeroes = (byte & ZEROES_MASK) + 2;
            ControlFlow::Break(IterItem {
                offset: 0,
                // SAFETY: `zeroes` cannot be zero, +2 could not overflow with MSBs masked away
                len: unsafe { NonZeroU8::new_unchecked(zeroes) },
            })
        }
        // 0b11_xxxxxx => wrapping byte of a multi-byte offset
        _ => ControlFlow::Continue((byte & WRAPPING_OFFSET_MASK) as u64),
    }
}

impl Iterator for OffsetIter<'_> {
    type Item = IterItem;
    fn next(&mut self) -> Option<Self::Item> {
        // check first byte
        let byte = *self.0.next()?;
        let mut offset = match check_first_byte(byte) {
            ControlFlow::Break(item) => return Some(item),
            ControlFlow::Continue(offset) => offset,
        };

        // this is a multi-byte offset

        let mut len = 1;

        // wrapping bytes contain 6 usable bits for the offset
        let mut shift = 6;

        loop {
            let byte = *self.0.next().unwrap(); // encoding expects more bytes to follow

            len += 1;

            if byte.leading_zeros() == 0 {
                // this is a wrapping byte

                // the added bits are more significant
                offset |= ((byte & WRAPPING_OFFSET_MASK) as u64) << shift;
                return Some(IterItem {
                    offset,
                    // SAFETY: len started with 1, could be at most 10, never overflows
                    len: unsafe { NonZeroU8::new_unchecked(len) },
                });
            }

            // this is a wrapped byte

            // the added bits are more significant, has no marker bits that need to be masked away
            offset |= (byte as u64) << shift;

            // wrapped bytes contain 7 usable bits for the offset
            shift += 7;
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.0.len();

        // at most 10 bytes are used to store a u64
        let min = len.div_ceil(10);

        // up to 65 zeroes can be stored in a byte
        let max = len.checked_mul(MAX_ZEROES_PER_BYTE as usize);

        (min, max)
    }
}

impl DoubleEndedIterator for OffsetIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        // check first byte
        let byte = *self.0.next_back()?;
        let mut offset = match check_first_byte(byte) {
            ControlFlow::Break(item) => return Some(item),
            ControlFlow::Continue(offset) => offset,
        };

        // this is a multi-byte offset

        let mut len = 1;

        loop {
            let byte = *self.0.next_back().unwrap(); // encoding expects more bytes to follow

            len += 1;

            if byte.leading_zeros() == 0 {
                // this is a wrapping byte

                // the added bits are less significant, wrapping bytes contain 6 usable bits for the
                // offset
                offset = (offset << 6) | (byte & WRAPPING_OFFSET_MASK) as u64;

                return Some(IterItem {
                    offset,
                    // SAFETY: len started with 1, could be at most 10, never overflows
                    len: unsafe { NonZeroU8::new_unchecked(len) },
                });
            }

            // this is a wrapped byte

            // the added bits are less significant, has no marker bits that need to be masked away,
            // wrapped bytes contain 7 usable bits for the offset
            offset = (offset << 7) | byte as u64;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn offset_iter_works() {
        let offsets = [
            0b___________________________________________________________________000000,
            0b____________________________________________________________000010_000001,
            0b____________________________________________________000101_0000100_000011,
            0b____________________________________________001001_0001000_0000111_000110,
            0b____________________________________001110_0001101_0001100_0001011_001010,
            0b____________________________010100_0010011_0010010_0010001_0010000_001111,
            0b____________________011011_0011010_0011001_0011000_0010111_0010110_010101,
            0b____________100011_0100010_0100001_0100000_0011111_0011110_0011101_011100,
            0b____101100_0101011_0101010_0101001_0101000_0100111_0100110_0100101_100100,
            0b10_0110101_0110100_0110011_0110010_0110001_0110000_0101111_0101110_101101,
        ];

        // 0b0xxxxxxx = x offset including zero
        // 0b10xxxxxx = x amount of zeroes + 1
        // 0b11xxxxxx = padding byte with x payload, wraps 0b0xxxxxxx bytes with x payload

        let deque: VecDeque<u8> = [
            // 0b000000
            0b0_0000000,
            //
            // 0b000010_000001
            0b11_000001,
            0b11_000010,
            //
            // 0b000101_0000100_000011
            0b11_000011,
            0b0_0000100,
            0b11_000101,
            //
            // 0b001001_0001000_0000111_000110
            0b11_000110,
            0b0_0000111,
            0b0_0001000,
            0b11_001001,
            //
            // 0b001110_0001101_0001100_0001011_001010
            0b11_001010,
            0b0_0001011,
            0b0_0001100,
            0b0_0001101,
            0b11_001110,
            //
            // 0b010100_0010011_0010010_0010001_0010000_001111
            0b11_001111,
            0b0_0010000,
            0b0_0010001,
            0b0_0010010,
            0b0_0010011,
            0b11_010100,
            //
            // 0b011011_0011010_0011001_0011000_0010111_0010110_010101
            0b11_010101,
            0b0_0010110,
            0b0_0010111,
            0b0_0011000,
            0b0_0011001,
            0b0_0011010,
            0b11_011011,
            //
            // 0b100011_0100010_0100001_0100000_0011111_0011110_0011101_011100
            0b11_011100,
            0b0_0011101,
            0b0_0011110,
            0b0_0011111,
            0b0_0100000,
            0b0_0100001,
            0b0_0100010,
            0b11_100011,
            //
            // 0b101100_0101011_0101010_0101001_0101000_0100111_0100110_0100101_100100
            0b11_100100,
            0b0_0100101,
            0b0_0100110,
            0b0_0100111,
            0b0_0101000,
            0b0_0101001,
            0b0_0101010,
            0b0_0101011,
            0b11_101100,
            //
            // 0b10_0110101_0110100_0110011_0110010_0110001_0110000_0101111_0101110_101101
            0b11_101101,
            0b0_0101110,
            0b0_0101111,
            0b0_0110000,
            0b0_0110001,
            0b0_0110010,
            0b0_0110011,
            0b0_0110100,
            0b0_0110101,
            0b11_0000_10, // only least significant two bits are available, more would overflow u64
            //
            // two following zeroes
            0b10_000000,
            //
            // 65 following zeroes
            MAX_ZEROES_AS_BYTE,
        ]
        .into();

        fn item(offset: u64, len: u8) -> IterItem {
            IterItem {
                offset,
                len: NonZeroU8::new(len).unwrap(),
            }
        }

        let expected = [
            // single byte value
            item(offsets[0], 1),
            // multi byte values
            item(offsets[1], 2),
            item(offsets[2], 3),
            item(offsets[3], 4),
            item(offsets[4], 5),
            item(offsets[5], 6),
            item(offsets[6], 7),
            item(offsets[7], 8),
            item(offsets[8], 9),
            item(offsets[9], 10),
            // 2 zeroes in a byte
            item(0, 2),
            // 65 zeroes in a byte
            item(0, MAX_ZEROES_PER_BYTE),
        ];

        assert!(OffsetIter(deque.iter()).eq(expected.iter().cloned()));

        assert!(
            OffsetIter(deque.iter())
                .rev()
                .eq(expected.into_iter().rev())
        );
    }
}
