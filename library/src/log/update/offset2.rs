use std::{
    borrow::Borrow,
    collections::{
        VecDeque,
        vec_deque::{Iter, IterMut},
    },
    ops::{ControlFlow, Deref},
};

/*
encapsule offset logic

challenges:
- distinguish peek/mutate in iter
-- mut if frame matches

man könnte zu wrapped offset noch + 64 rechnen

available bits n * 7 + 12
*/

const WRAPPED_OFFSET_MASK: u8 = 0b0_1111111;
const MAX_SINGLE_BYTE_OFFSET: u64 = 0b00_111111 + 2;
const MAX_OFFSETS_PER_STRAKE_BYTE: u64 = MAX_SINGLE_BYTE_OFFSET + 2;
const NON_WRAPPED_BYTE_MASK: u8 = 0b00_111111;
const WRAPPING_OFFSET_OR: u8 = 0b11_000000;
const MAX_WRAPPING_OFFSET: u8 = 0b00_111111;

const MULTI_BYTE_OFFSETS: [u64; 9] = [
    66,
    4162,
    528450,
    67637314,
    8657571906,
    1108169199682,
    141845657555010,
    18156244167036994,
    2323999253380730946,
];

#[derive(Debug, Default, Clone)]
pub(super) struct OffsetLog {
    offsets: VecDeque<u8>,
    meta: OffsetMeta,
}

impl OffsetLog {
    pub(super) fn now_to_future(&mut self) -> OffsetIter<true> {
        self.log_iter()
    }
    pub(super) fn now_to_past(&mut self) -> OffsetIter<false> {
        self.log_iter()
    }
    fn log_iter<const FORWARD: bool>(&mut self) -> OffsetIter<FORWARD> {
        OffsetIter {
            iter: if FORWARD {
                self.offsets.range(self.meta.index..)
            } else if self.meta.streak_and_step.is_some() {
                self.offsets.range(..(self.meta.index - 1))
            } else {
                self.offsets.range(..self.meta.index)
            },
            meta: self.meta,
            meta_mut: &mut self.meta,
            early_next_strake: None
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct OffsetMeta {
    index: usize,
    streak_and_step: Option<(Streak, u8)>,
}

#[derive(Debug)]
pub(super) struct OffsetIter<'a, const FORWARD: bool> {
    iter: Iter<'a, u8>,
    meta: OffsetMeta,
    meta_mut: &'a mut OffsetMeta,
    streak_set_pending: bool
}

impl<const FORWARD: bool> OffsetIter<'_, FORWARD> {
    pub(super) fn sync(&mut self) {
        *self.meta_mut = self.meta;
    }
}

impl OffsetIter<'_, false> {
    fn set_pending_streak(&mut self) {
        if self.streak_set_pending {
            self.streak_set_pending = false;
            if let Some(FirstByte::SingleByteStreak(streak)) = self.iter.clone().next_back().map(FirstByte::from) {
                self.iter.next_back();
                self.meta.index -= 1;
                self.meta.streak_and_step = Some((streak, streak.max));
            }
        }
    }
}

impl<const FORWARD: bool> Iterator for OffsetIter<'_, FORWARD> {
    type Item = u64;
    fn next(&mut self) -> Option<Self::Item> {
        if FORWARD {
            let first_byte;

            match self.meta.streak_and_step {
                Some((Streak { max, one }, ref mut step)) => {
                    if *step < max {
                        *step += 1;
                        return Some(one as u64);
                    }
                    first_byte = self.iter.next()?;
                    self.meta.streak_and_step = None;
                }
                None => first_byte = self.iter.next()?,
            }

            self.meta.index += 1;

            let mut offset = match FirstByte::from(first_byte) {
                FirstByte::SingleByteOffset(offset) => return Some(offset),
                FirstByte::SingleByteStreak(streak) => {
                    self.meta.streak_and_step = Some((streak, 0));
                    return Some(streak.one as u64);
                }
                FirstByte::MultiByteOffsetIncomplete(offset) => offset,
            };

            let mut extra_offsets_index = 0;

            // this is a multi-byte offset

            // wrapping bytes contain 6 usable bits for the offset
            let mut shift = 6;

            loop {
                let byte = *self.iter.next().unwrap(); // encoding expects more bytes to follow

                self.meta.index += 1;

                if byte.leading_zeros() == 0 {
                    // this is a wrapping byte

                    // the added bits are more significant
                    offset |= ((byte & NON_WRAPPED_BYTE_MASK) as u64) << shift;

                    // multi-byte offsets are always at least MULTI_BYTE_OFFSET, so the encoded
                    // offset is less than that to potenitally require a byte less
                    offset += MULTI_BYTE_OFFSETS[extra_offsets_index];

                    return Some(offset);
                }

                // this is a wrapped byte

                // the added bits are more significant, has no marker bits that need to be masked away
                offset |= (byte as u64) << shift;

                // wrapped bytes contain 7 usable bits for the offset
                shift += 7;

                extra_offsets_index += 1;
            }
        } else {
            self.set_pending_streak();
            if let Some((Streak { one, .. }, ref mut step)) = self.meta.streak_and_step {
                
                return Some(one as u64);
            }

            let first_byte = self.iter.next_back()?;
            self.meta.index -= 1;

            let mut offset = match FirstByte::from(first_byte) {
                FirstByte::SingleByteOffset(offset) => {
                    return Some(offset);
                }
                FirstByte::SingleByteStreak(streak) => {
                    if streak.max != 0 {
                        self.meta.streak_and_step = Some((streak, streak.max - 1));
                    } else if let Some(FirstByte::SingleByteStreak(streak)) = self.iter.clone().next_back().map(FirstByte::from) {
                        self.iter.next_back();
                        self.meta.streak_and_step = Some((streak, streak.max));
                        self.meta_streak_some_early = true;
                    }
                    return Some(streak.one as u64);
                }
                FirstByte::MultiByteOffsetIncomplete(offset) => offset,
            };

            let mut extra_offsets_index = 0;

            loop {
                let byte = *self.iter.next_back().unwrap(); // encoding expects more bytes to follow

                self.meta.index -= 1;

                if byte.leading_zeros() == 0 {
                    // this is a wrapping byte

                    // the added bits are less significant, wrapping bytes contain 6 usable bits for the
                    // offset
                    offset = (offset << 6) | (byte & NON_WRAPPED_BYTE_MASK) as u64;

                    // multi-byte offsets are always at least MULTI_BYTE_OFFSET, so the encoded
                    // offset is less than that to potenitally require a byte less
                    offset += MULTI_BYTE_OFFSETS[extra_offsets_index];

                    return Some(offset);
                }

                // this is a wrapped byte

                // the added bits are less significant, has no marker bits that need to be masked away,
                // wrapped bytes contain 7 usable bits for the offset
                offset = (offset << 7) | byte as u64;

                extra_offsets_index += 1;
            }
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.iter.len();

        // at most 10 bytes are used to store a u64
        let min = len.div_ceil(10);

        // up to 64 zeroes or ones can be stored in a byte
        let max = len.checked_mul(64);

        (min, max)
    }
}

#[derive(Clone, Copy, Debug)]
struct Streak {
    max: u8,
    one: bool,
}

enum FirstByte {
    SingleByteOffset(u64),
    SingleByteStreak(Streak),
    MultiByteOffsetIncomplete(u64),
}

impl From<&u8> for FirstByte {
    fn from(value: &u8) -> Self {
        let value = *value;
        match value >> 6 {
            0b00 => Self::SingleByteOffset(value as u64 + 2), // 0 and 1 are encoded in streaks, so + 2 here
            0b01 => Self::SingleByteStreak(Streak {
                max: value & NON_WRAPPED_BYTE_MASK,
                one: false
            }),
            0b10 => Self::SingleByteStreak(Streak {
                max: value & NON_WRAPPED_BYTE_MASK,
                one: true
            }),
            _ /*0b11*/ => Self::MultiByteOffsetIncomplete((value & NON_WRAPPED_BYTE_MASK) as u64)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[rustfmt::skip]
    const OFFSETS_ENCODED: [u8; 114] = [
        0b00_000000,
        0b00_111111,
        0b01_000000,
        0b10_000000,
        0b01_111111,
        0b10_111111,
        0b11_000000, 0b11_000000,
        0b11_111111, 0b11_111111,
        0b11_000000, 0x00, 0b11_000000,
        0b11_111111, 0x7F, 0b11_111111,
        0b11_000000, 0x00, 0x00, 0b11_000000,
        0b11_111111, 0x7F, 0x7F, 0b11_111111,
        0b11_000000, 0x00, 0x00, 0x00, 0b11_000000,
        0b11_111111, 0x7F, 0x7F, 0x7F, 0b11_111111,
        0b11_000000, 0x00, 0x00, 0x00, 0x00, 0b11_000000,
        0b11_111111, 0x7F, 0x7F, 0x7F, 0x7F, 0b11_111111,
        0b11_000000, 0x00, 0x00, 0x00, 0x00, 0x00, 0b11_000000,
        0b11_111111, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0b11_111111,
        0b11_000000, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0b11_000000,
        0b11_111111, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0b11_111111,
        0b11_000000, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0b11_000000,
        0b11_111111, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0b11_111111,
        0b11_000000, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0b11_000000,
        0b11_111101, 0x3E, 0x3F, 0x3F, 0x3F, 0x3F, 0x3F, 0x3F, 0x3F, 0b11_000011,
    ];
    #[rustfmt::skip]
    const OFFSETS_DECODED: [u64; 150] = [
        2, 65, 0, 1,
        // x64
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // x64
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        66, 4161,
        4162, 528449,
        528450, 67637313,
        67637314, 8657571905,
        8657571906, 1108169199681,
        1108169199682, 141845657555009,
        141845657555010, 18156244167036993,
        18156244167036994, 2323999253380730945,
        2323999253380730946, u64::MAX
    ];

    #[test]
    fn now_to_future_works() {
        let mut log = OffsetLog {
            offsets: OFFSETS_ENCODED.into(),
            meta: OffsetMeta::default(),
        };
        let actual = log.now_to_future().collect::<Vec<_>>();
        assert_eq!(actual, OFFSETS_DECODED)
    }

    #[test] // todo: more complex Offsetmeta
    fn now_to_past_works() {
        let mut log = OffsetLog {
            offsets: OFFSETS_ENCODED.into(),
            meta: OffsetMeta::default(),
        };

        // set to future end
        let mut iter = log.now_to_future();
        for _ in &mut iter {}
        iter.sync();

        let actual = log.now_to_past().collect::<Vec<_>>();
        let mut expected = OFFSETS_DECODED;
        expected.reverse();
        assert_eq!(actual, expected)
    }

    #[test]
    fn offset_iter_sync_works() {
        let mut log = OffsetLog {
            offsets: OFFSETS_ENCODED.into(),
            meta: OffsetMeta::default(),
        };

        // from start to some end and back
        for steps in 0..OFFSETS_ENCODED.len() {
            let mut iter = log.now_to_future();
            for index in 0..steps {
                let before = (iter.iter.clone(), iter.meta);
                let actual = iter.next().unwrap();
                assert_eq!(
                    actual, OFFSETS_DECODED[index],
                    "index: {index}, steps: {steps}, before: {before:?}"
                );
                print!("{index}, ");
            }
            iter.sync();
            println!();

            let mut iter = log.now_to_past();
            for index in (0..steps).rev() {
                let before = (iter.iter.clone(), iter.meta);
                if steps == 4 {
                    println!("{before:#?}");
                }
                let actual = iter.next().unwrap();
                assert_eq!(
                    actual, OFFSETS_DECODED[index],
                    "index: {index}, steps: {steps}"
                );
                print!("{index}, ");
            }
            iter.sync();
            println!();
        }

        // set to future end
        let mut iter = log.now_to_future();
        for _ in &mut iter {}
        iter.sync();

        // from end to some start and back
        for steps in 0..OFFSETS_ENCODED.len() {
            let min = OFFSETS_ENCODED.len() - steps;

            let mut iter = log.now_to_past();
            for index in (0..steps).rev() {
                let before = (iter.iter.clone(), iter.meta);
                let actual = iter.next().unwrap();
                assert_eq!(
                    actual,
                    OFFSETS_DECODED[index + min],
                    "index: {index}, steps: {steps}, before: {before:#?}"
                );
            }
            iter.sync();

            let mut iter = log.now_to_future();
            for index in 0..steps {
                let before = (iter.iter.clone(), iter.meta);
                let actual = iter.next().unwrap();
                assert_eq!(
                    actual,
                    OFFSETS_DECODED[index + min],
                    "index: {index}, steps: {steps}, before: {before:#?}"
                );
            }
            iter.sync();
        }
    }
}
