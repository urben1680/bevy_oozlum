use core::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::{TryReserveError, VecDeque},
    error::Error,
    fmt::Display, ops::ControlFlow,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

#[derive(Clone, Default, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrameTransitionLog {
    // 0b00xxxxxx = 1 byte, 0b01xxxxxx = 1+n+1 bytes where n are 0b1xxxxxxx
    offset_bytes: VecDeque<u8>,
    /// A frame this log matched at (or 0) that is either at [`RevMeta::past_end`] or as closely before it as possible
    /// to determine how much other logs that run along this can be reduced to.
    out_of_or_past_end_log: u64,
    last_run: u64,
    index: usize,
    past_len: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::FrameTransitionLog;

    impl WithCapacity for FrameTransitionLog {
        type Se<'se> = (
            WithCapacityWrapper<&'se VecDeque<u8>>,
            u64,
            u64,
            usize,
            usize,
        );
        type De = (WithCapacityWrapper<VecDeque<u8>>, u64, u64, usize, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                WithCapacityWrapper(&self.offset_bytes),
                self.out_of_or_past_end_log,
                self.last_run,
                self.index,
                self.past_len,
            )
        }
        fn from_with_capacity(
            (WithCapacityWrapper(offset_bytes), out_of_or_past_end_log, last_run, index, past_len): Self::De,
        ) -> Self {
            Self {
                offset_bytes,
                out_of_or_past_end_log,
                last_run,
                index,
                past_len,
            }
        }
    }

    impl LoglessWithCapacity for FrameTransitionLog {
        type Se<'se> = usize;
        type De = usize;
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            self.bytes_capacity()
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
    }
}

impl Debug for FrameTransitionLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameTransitionLog")
            .field("offset_bytes", &self.offset_bytes)
            .field(
                "offsets()",
                &Iter::from(self.offset_bytes.iter()),
            )
            .field("out_of_or_past_end_log", &self.out_of_or_past_end_log)
            .field("last_run", &self.last_run)
            .field("index", &self.index)
            .field("past_len", &self.past_len)
            .finish()
    }
}

#[derive(Clone)]
struct Iter<'a> {
    iter: std::collections::vec_deque::Iter<'a, u8>,
    zeroes: u8
}

#[derive(Debug, PartialEq)]
struct IterItem {
    offset: u64,
    len: usize
}

impl Iter<'_> {
    fn check_zeroes(&mut self) -> ControlFlow<IterItem> {
        match self.zeroes {
            0 => ControlFlow::Continue(()),
            1 => {
                self.zeroes = 0;
                self.iter.next(); // advance iter to not run into the same zeroes again
                ControlFlow::Break(IterItem {
                    offset: 0,
                    len: 1 // now advance index
                })
            },
            _ => {
                self.zeroes -= 1;
                ControlFlow::Break(IterItem {
                    offset: 0,
                    len: 0 // do not advance index
                })
            }
        }
    }
    fn check_first_byte(&mut self, byte: u8) -> ControlFlow<IterItem, u64> {
        match byte.leading_zeros() {
            0 => {
                // byte = 0b1_11111111 -> self.zeroes = 1
                // byte = 0b1_11111110 -> self.zeroes = 2
                // ...
                // byte = 0b1_00000000 -> self.zeroes = 128
                self.zeroes = !byte + 1;
                ControlFlow::Break(IterItem {
                    offset: 0,
                    len: 0 // do not advance index
                })
            }
            1 => ControlFlow::Continue((byte & 0b00_111111) as u64),
            _ => ControlFlow::Break(IterItem {
                offset: byte as u64,
                len: 1,
            })
        }
    }
}

impl<'a> From<std::collections::vec_deque::Iter<'a, u8>> for Iter<'a> {
    fn from(iter: std::collections::vec_deque::Iter<'a, u8>) -> Self {
        Self {
            iter,
            zeroes: 0
        }
    }
}

impl Debug for Iter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.clone().map(|item| item.offset))
            .finish()
    }
}

impl Iterator for Iter<'_> {
    type Item = IterItem;
    fn next(&mut self) -> Option<Self::Item> {
        if let ControlFlow::Break(item) = self.check_zeroes() {
            return Some(item);
        }
        let byte = *self.iter.next()?;
        let mut frame = match self.check_first_byte(byte) {
            ControlFlow::Break(item) => return Some(item),
            ControlFlow::Continue(frame) => frame
        };
        let mut len = 1;
        let mut shift = 6;
        loop {
            let byte = *self.iter.next().unwrap(); // encoding expects more bytes to follow until MSB is 0
            len += 1;
            if byte.leading_zeros() != 0 {
                let offset = frame | (((byte & 0b00_111111) as u64) << shift);
                return Some(IterItem {
                    offset,
                    len,
                });
            }
            frame |= ((byte & 0b0_1111111) as u64) << shift;
            shift += 7;
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.iter.len() {
            0 => (0, Some(0)),
            // at most 10 bytes are used to store a u64
            len => (len.div_ceil(10), Some(len))
        }
    }
}

impl DoubleEndedIterator for Iter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if let ControlFlow::Break(item) = self.check_zeroes() {
            return Some(item);
        }
        let byte = *self.iter.next_back()?;
        let mut frame = match self.check_first_byte(byte) {
            ControlFlow::Break(item) => return Some(item),
            ControlFlow::Continue(frame) => frame
        };
        let mut len = 1;
        loop {
            len += 1;
            let byte = *self.iter.next_back().unwrap(); // encoding expects more bytes to follow until MSB is 0
            if byte.leading_zeros() != 0 {
                let offset = (frame << 6) | (byte & 0b00_111111) as u64;
                return Some(IterItem {
                    offset,
                    len,
                });
            }
            frame = (frame << 7) | (byte & 0b0_1111111) as u64;
        }
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissedFrame(pub u64);

impl Display for MissedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the expected frame {} was missed", self.0)
    }
}

impl Error for MissedFrame {}

impl FrameTransitionLog {
    pub const fn new() -> Self {
        Self {
            offset_bytes: VecDeque::new(),
            out_of_or_past_end_log: 0,
            last_run: 0,
            index: 0,
            past_len: 0,
        }
    }
    pub fn with_capacity(bytes_capacity: usize) -> Self {
        Self {
            offset_bytes: VecDeque::with_capacity(bytes_capacity),
            out_of_or_past_end_log: 0,
            last_run: 0,
            index: 0,
            past_len: 0,
        }
    }
    pub fn bytes_len(&self) -> usize {
        self.offset_bytes.len()
    }
    pub fn bytes_capacity(&self) -> usize {
        self.offset_bytes.capacity()
    }
    pub fn bytes_is_empty(&self) -> bool {
        self.offset_bytes.is_empty()
    }
    pub fn bytes_reserve(&mut self, additional: usize) {
        self.offset_bytes.reserve(additional)
    }
    pub fn bytes_reserve_exact(&mut self, additional: usize) {
        self.offset_bytes.reserve_exact(additional)
    }
    pub fn bytes_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve(additional)
    }
    pub fn bytes_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve_exact(additional)
    }
    pub fn bytes_shrink_to(&mut self, min_capacity: usize) {
        self.offset_bytes.shrink_to(min_capacity)
    }
    pub fn bytes_shrink_to_fit(&mut self) {
        self.offset_bytes.shrink_to_fit()
    }
    pub fn truncate_future(&mut self) {
        self.offset_bytes.truncate(self.index);
    }
    pub fn clear(&mut self) {
        self.offset_bytes.clear();
        self.index = 0;
    }
    pub fn push_and_get_past_len(&mut self, meta: &RevMeta) -> usize {
        if !self.offset_bytes.is_empty() {
            // truncate future
            self.truncate_future();

            // truncate past

            // NOTE: self.out_of_or_past_end_log must remain actually unreachable for this as iterating backward is only
            // possible if the current self.index can be reduced when self.last_run matches. This would not be the case
            // for when self.out_of_or_past_end_log is reached as then self.index can* be 0 and cannot be further reduced.
            // If reducing is not done in that case to support this frame to match, it would not be able to detect that yet
            // another run at this frame as not-matching without adding another bool field to the log.
            //
            // *self.index may be highter when there are offsets of 0 at the start of the log. This however should not
            // occur in practice as the default value is 0 and due to RevMeta::update implementation, the lowest frame to
            // run RevUpdate at is at frame 1. Otherwise, the below truncating will also remove offsets of 0 at the start
            // if present.

            let iter = Iter::from(self.offset_bytes.iter());

            let mut to_drain = 0;

            for IterItem { offset, len } in iter {
                let next_oldest = self.out_of_or_past_end_log + offset;
                if next_oldest > meta.past_end() {
                    break;
                }
                to_drain += len;
                self.out_of_or_past_end_log = next_oldest;
                self.past_len -= 1;
            }

            self.index -= to_drain;
            self.offset_bytes.drain(..to_drain); // https://github.com/rust-lang/rust/issues/140667
        }

        // push present offset
        // todo: impl increase multi-zero offset
        let mut offset = meta.now() - self.last_run;
        self.last_run = meta.now();
        self.past_len += 1;
        self.index += 1;

        if offset <= 0b00_111111 {
            self.offset_bytes.push_back(offset as u8);
            return self.past_len;
        }

        self.offset_bytes
            .push_back((offset & 0b00_111111) as u8 | 0b01_000000);
        offset >>= 6;

        loop {
            self.index += 1;
            if offset <= 0b00_111111 {
                self.offset_bytes.push_back(offset as u8 | 0b01_000000);
                return self.past_len;
            }
            self.offset_bytes
                .push_back((offset & 0b0_1111111) as u8 | 0b1_0000000);
            offset >>= 7;
        }
    }
    pub fn try_backward_log(&mut self, meta: &RevMeta) -> Result<bool, MissedFrame> {
        let Some(IterItem { offset, len }) = Iter::from(self.offset_bytes.range(..self.index)).next_back() else {
            return Ok(false);
        };
        match self.last_run.cmp(&(meta.now() + 1)) {
            Ordering::Less => Ok(false),
            Ordering::Equal => {
                self.last_run -= offset;
                self.index -= len;
                self.past_len -= 1;
                Ok(true)
            }
            Ordering::Greater => Err(MissedFrame(self.last_run)),
        }
    }
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        self.try_backward_log(meta).unwrap()
    }
    pub fn try_forward_log(&mut self, meta: &RevMeta) -> Result<bool, MissedFrame> {
        let Some(IterItem { offset, len }) = Iter::from(self.offset_bytes.range(self.index..)).next() else {
            return Ok(false);
        };
        let frame = self.last_run + offset;
        match frame.cmp(&meta.now()) {
            Ordering::Greater => Ok(false),
            Ordering::Equal => {
                self.last_run = frame;
                self.index += len;
                self.past_len += 1;
                Ok(true)
            }
            Ordering::Less => Err(MissedFrame(frame)),
        }
    }
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        self.try_forward_log(meta).unwrap()
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU64;

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

        let deque: VecDeque<u8> = [
            // 0b000000
            0b00_000000,
            //
            // 0b000010_000001
            0b01_000001,
            0b01_000010,
            //
            // 0b000101_0000100_000011
            0b01_000011,
            0b1_0000100,
            0b01_000101,
            //
            // 0b001001_0001000_0000111_000110
            0b01_000110,
            0b1_0000111,
            0b1_0001000,
            0b01_001001,
            //
            // 0b001110_0001101_0001100_0001011_001010
            0b01_001010,
            0b1_0001011,
            0b1_0001100,
            0b1_0001101,
            0b01_001110,
            //
            // 0b010100_0010011_0010010_0010001_0010000_001111
            0b01_001111,
            0b1_0010000,
            0b1_0010001,
            0b1_0010010,
            0b1_0010011,
            0b01_010100,
            //
            // 0b011011_0011010_0011001_0011000_0010111_0010110_010101
            0b01_010101,
            0b1_0010110,
            0b1_0010111,
            0b1_0011000,
            0b1_0011001,
            0b1_0011010,
            0b01_011011,
            //
            // 0b100011_0100010_0100001_0100000_0011111_0011110_0011101_011100
            0b01_011100,
            0b1_0011101,
            0b1_0011110,
            0b1_0011111,
            0b1_0100000,
            0b1_0100001,
            0b1_0100010,
            0b01_100011,
            //
            // 0b101100_0101011_0101010_0101001_0101000_0100111_0100110_0100101_100100
            0b01_100100,
            0b1_0100101,
            0b1_0100110,
            0b1_0100111,
            0b1_0101000,
            0b1_0101001,
            0b1_0101010,
            0b1_0101011,
            0b01_101100,
            //
            // 0b10_0110101_0110100_0110011_0110010_0110001_0110000_0101111_0101110_101101
            0b01_101101,
            0b1_0101110,
            0b1_0101111,
            0b1_0110000,
            0b1_0110001,
            0b1_0110010,
            0b1_0110011,
            0b1_0110100,
            0b1_0110101,
            0b01_0000_10, // only least significant two bits are available, more would overflow u64
            //
            // two following zeroes
            0b1_1111111,
            //
            // separator
            0b00_000001,
            //
            // three following zeroes
            0b1_1111110,
            //
            // separator
            0b00_000001,
            //
            // 129 following zeroes
            0b1_0000000
        ]
        .into();

        assert_eq!(
            Iter::from(deque.iter()).collect::<Vec<_>>(),
            [
                // single byte value
                IterItem { offset: offsets[0], len: 1 },
                // multi byte values
                IterItem { offset: offsets[1], len: 2 },
                IterItem { offset: offsets[2], len: 3 },
                IterItem { offset: offsets[3], len: 4 },
                IterItem { offset: offsets[4], len: 5 },
                IterItem { offset: offsets[5], len: 6 },
                IterItem { offset: offsets[6], len: 7 },
                IterItem { offset: offsets[7], len: 8 },
                IterItem { offset: offsets[8], len: 9 },
                IterItem { offset: offsets[9], len: 10 },
                // two following zeroes
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 1 },
                // separator
                IterItem { offset: 1, len: 1 },
                // three following zeroes
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 1 },
                // separator
                IterItem { offset: 1, len: 1 },
                // 129 following zeroes
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 1 },
            ]
        );

        assert_eq!(
            Iter::from(deque.iter()).rev().collect::<Vec<_>>(),
            [
                // 129 following zeroes
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 1 },
                // separator
                IterItem { offset: 1, len: 1 },
                // three following zeroes
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 1 },
                // separator
                IterItem { offset: 1, len: 1 },
                // two following zeroes
                IterItem { offset: 0, len: 0 },
                IterItem { offset: 0, len: 1 },
                // multi byte values
                IterItem { offset: offsets[9], len: 10 },
                IterItem { offset: offsets[8], len: 9 },
                IterItem { offset: offsets[7], len: 8 },
                IterItem { offset: offsets[6], len: 7 },
                IterItem { offset: offsets[5], len: 6 },
                IterItem { offset: offsets[4], len: 5 },
                IterItem { offset: offsets[3], len: 4 },
                IterItem { offset: offsets[2], len: 3 },
                IterItem { offset: offsets[1], len: 2 },
                // single byte value
                IterItem { offset: offsets[0], len: 1 },
            ]
        );
    }

    struct Log {
        log: FrameTransitionLog,
        meta: RevMeta,
    }

    impl Log {
        fn new(max_world_states: u64, now: u64) -> Self {
            let log = FrameTransitionLog::new();
            let meta = RevMeta::new(NonZeroU64::new(max_world_states), now, false);
            Self { log, meta }
        }
        fn forward(&mut self, updates_with_expected_past_len: Vec<usize>) {
            self.meta.queue_not_log_forward();
            self.meta
                .update(|meta| {
                    let before = self.log.clone();
                    let len = updates_with_expected_past_len.len();
                    let updates_with_actual_past_len: Vec<usize> = (0..len)
                        .map(|_| self.log.push_and_get_past_len(meta))
                        .collect();
                    assert_eq!(
                        updates_with_actual_past_len, updates_with_expected_past_len,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    )
                })
                .expect("should update");
        }
        fn forward_log(&mut self, expected_forward_log_updates: usize) {
            let previous = self.meta.now() + 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| {
                    for _ in 0..expected_forward_log_updates {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.forward_log(meta),
                            true,
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                            self.log
                        );
                    }
                    let before = self.log.clone();
                    assert_eq!(
                        self.log.forward_log(meta),
                        false,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    );
                })
                .expect("should update");
        }
        fn backward_log(&mut self, expected_backward_log_updates: usize) {
            let previous = self.meta.now() - 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| {
                    for _ in 0..expected_backward_log_updates {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.backward_log(meta),
                            true,
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                            self.log
                        );
                    }
                    let before = self.log.clone();
                    assert_eq!(
                        self.log.backward_log(meta),
                        false,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    );
                })
                .expect("should update");
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut log = Log::new(4, 0);
        log.forward(vec![1]); // frame #1
        log.forward(vec![2, 3]); // frame #2
        log.forward(vec![4]);
        log.forward(vec![]);
        // shortened log of runs from frame #1 and #2
        log.forward(vec![2, 3]); // !! this changed from vec![4, 5]

        log.backward_log(2);
        log.backward_log(0);
        log.backward_log(1);

        log.forward_log(1);
        log.forward_log(0);
        log.forward_log(2);

        log.backward_log(2);
        log.backward_log(0);
        log.backward_log(1);

        log.forward(vec![1]);
    }

    #[test]
    fn behaves_like_meta_if_updated_once_per_frame() {
        let mut log = Log::new(4, 0);

        log.forward(vec![1]); // frame #1
        assert_eq!(log.meta.past_len(), 1);

        log.forward(vec![2]); // frame #2
        assert_eq!(log.meta.past_len(), 2);

        log.forward(vec![3]);
        assert_eq!(log.meta.past_len(), 3);

        log.forward(vec![3]);
        assert_eq!(log.meta.past_len(), 3);
    }
}
