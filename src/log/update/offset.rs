//! This module contains a specialized [`OffsetLog`] that is used by
//! [`UpdateLog`](super::UpdateLog) to translate stored offsets into frames.
//!
//! # Motivation
//!
//! `UpdateLog` needs to keep track at which frame it has been updated. A very simple implementation
//! of this could be if it contained a `VecDeque<u64>`. However, this would would grow by eight
//! bytes per update. Updates that happen almost every frame or even many times per frame would
//! consume a large amount of data very quickly.
//!
//! # Memory optimizations
//!
//! Instead of storing the frames `UpdateLog` ran, it stores the amount of frames since the last
//! update. As these "offsets" most often fit in far smaller integer types than frames, this enables
//! multiple memory optimizations:
//!
//! ## 1. Variable length of stored integers
//!
//! Small offsets should not be stored as `u64` values as that would give no benefits. Instead,
//! they should not consume more bytes than needed to store them.
//!
//! Since the bytes also need to store some kind of flag that indicates how many bytes an offset
//! is made of, this cannot be 100% efficient. For example, an offset of `255` requires more than
//! one byte. Still, offsets of up to `65` do fit into a single byte, followed by `4161` in two
//! bytes, `528449` in three bytes, etc.
//!
//! ## 2. Storing multiple offsets of `1` in a single byte
//!
//! It is expected that many `UpdateLog`s still are mostly updated once every frame. So the encoding
//! packs this offset more tightly than larger offsets. Streaks of up to `64` offsets of `1` fit
//! into a single byte.
//!
//! An `UpdateLog` that is updated every frame is 512 times smaller than a naive `VecDeque<u64>`
//! implementation.
//!
//! ## 3. Storing multiple offsets of `0` in a single byte
//!
//! Other use cases of `UpdateLog` can be if it is updated (much) more often than once per frame.
//! This case should also consume not too much more. So an identical approach as `1`-offset streaks
//! is used here.
//!
//! # Computational costs
//!
//! Dealing with offsets instead of simple frames has its computational costs. It is attempted here
//! to minimize that for the most frequent updates, namely streaks of `0` and `1` and truncations of
//! small amounts of offsets at the front of the log if they become globally out-of-log.

use alloc::collections::{
    VecDeque,
    vec_deque::{Iter, IterMut},
};
use core::borrow::Borrow;

const WRAPPED_OFFSET_MASK: u8 = 0b0_1111111;
const NON_WRAPPED_BYTE_MASK: u8 = 0b00_111111;
const WRAPPING_OFFSET_OR: u8 = 0b11_000000;
const STREAK_ZERO_OR: u8 = 0b01_000000;
const STREAK_ONE_OR: u8 = 0b10_000000;
const SINGLE_BYTE_OFFSET: u64 = 2;
const MULTI_BYTE_OFFSET_0: u64 = 66;
const MULTI_BYTE_OFFSET_1: u64 = 4162;
const MULTI_BYTE_OFFSET_2: u64 = 528450;
const MULTI_BYTE_OFFSET_3: u64 = 67637314;
const MULTI_BYTE_OFFSET_4: u64 = 8657571906;
const MULTI_BYTE_OFFSET_5: u64 = 1108169199682;
const MULTI_BYTE_OFFSET_6: u64 = 141845657555010;
const MULTI_BYTE_OFFSET_7: u64 = 18156244167036994;
const MULTI_BYTE_OFFSET_8: u64 = 2323999253380730946;
const MULTI_BYTE_OFFSETS: [u64; 9] = [
    MULTI_BYTE_OFFSET_0,
    MULTI_BYTE_OFFSET_1,
    MULTI_BYTE_OFFSET_2,
    MULTI_BYTE_OFFSET_3,
    MULTI_BYTE_OFFSET_4,
    MULTI_BYTE_OFFSET_5,
    MULTI_BYTE_OFFSET_6,
    MULTI_BYTE_OFFSET_7,
    MULTI_BYTE_OFFSET_8,
];

/// Stores `u64` offsets and allows forward/backward traversals and past/future truncations.
#[derive(Debug, Default)]
pub(super) struct OffsetLog {
    /// Encoded offsets in the log, see module docs for a summary on the encoding.
    ///
    /// # Encoding
    ///
    /// - Streaks of `0` are encoded in `0b01_xxxxxx` where `x` are available for storing an integer
    ///   of up to `63`. Since there is no concept of "zero times an offset of `0`", a byte of
    ///   `0b01_000000` is interpreted as "one time an offset of `0`" instead.
    /// - Streaks of `1` are encoded in `0b10_xxxxxx` and work otherwise in the same way as streaks
    ///   of `0` do.
    /// - Offsets of `2` to `65` are encoded in `0b00_xxxxxx`. Since offsets of `0` and `1` are
    ///   already covered by streak encodings, a byte of `0b00_000000` is interpreted as "an offset
    ///   of `2`".
    /// - Offsets stored in multiple bytes are encoded in `0b11_xxxxxx` for the first and last byte
    ///   and `0b0_xxxxxxx` for all bytes in between, if any are needed. As `UpdateLog` needs to be
    ///   able to read the offset backwards too, the last byte needs to contain the two flag bits as
    ///   well.
    /// - Depending on how many in-between "wrapped" bytes there are, a constant value is added to
    ///   the offset. This way, `[0b11_000000, 0b11_000000]` and
    ///   `[0b11_000000, 0b0_0000000, 0b11_000000]` decode to different offsets despite the payload
    ///   bits combine to the same integer. This further compresses the offset values into
    ///   potentially less bytes.
    offsets: VecDeque<u8>,

    /// The current position in the log, determining where the past segment ends and the future
    /// segment starts.
    meta: OffsetMeta,
}

impl OffsetLog {
    pub(super) const fn new() -> Self {
        Self {
            offsets: VecDeque::new(),
            meta: OffsetMeta {
                index: 0,
                streak_and_step: None,
            },
        }
    }

    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            offsets: VecDeque::with_capacity(capacity),
            meta: OffsetMeta::default(),
        }
    }

    /// Only for capacity getters.
    pub(super) fn get_bytes(&self) -> &VecDeque<u8> {
        &self.offsets
    }

    /// Only for capacity setters.
    pub(super) fn get_bytes_mut(&mut self) -> &mut VecDeque<u8> {
        &mut self.offsets
    }

    /// Returns an iterator that yields the decoded offsets as `u64`, going from the chronologically
    /// next offsets further into the future.
    ///
    /// Use [`NowToFuture::sync`] to synchronize the log position with the iterator.
    pub(super) fn now_to_future<'a>(&'a mut self) -> NowToFuture<'a> {
        NowToFuture {
            iter: self.offsets.range(self.meta.index..),
            meta: self.meta,
            meta_mut: &mut self.meta,
        }
    }

    /// Returns an iterator that yields the decoded offsets as `u64`, going from the chronologically
    /// previous offsets further into the past.
    ///
    /// Use [`NowToPast::sync`] to synchronize the log position with the iterator.
    pub(super) fn now_to_past<'a>(&'a mut self) -> NowToPast<'a> {
        let mut to = self.meta.index;
        if self.meta.streak_and_step.is_some() {
            // The iterator uses `Streak` to yield multiple items for streaks without progressing
            // the inner iterator. If this starts with some `Streak`, this basically means the
            // youngest offset was already read and the iterator should skip it when the streak is
            // exhausted.
            // This should never underflow because `streak_and_step` should be set to `None` when
            // `index` is decreased to `0`.
            to -= 1;
        }
        NowToPast {
            iter: self.offsets.range(..to),
            meta: self.meta,
            meta_mut: &mut self.meta,
            streak_set_pending: false,
        }
    }

    /// Remove all future offsets from the log.
    pub(super) fn truncate_future(&mut self) {
        self.offsets.truncate(self.meta.index);
        if let Some((ref mut streak, step)) = self.meta.streak_and_step
            && streak.max != step
        {
            streak.max = step;
            // expects an offset to exist when streak_and_step is Some
            *self.offsets.back_mut().unwrap() = (*streak).into();
        }
    }

    /// Truncates all past offsets that sum up to `minus`.
    ///
    /// This method returns the amount of offsets that were truncated.
    ///
    /// The first offset that, when added to the previously truncated offsets, exceeds `minus`, will
    /// be kept. If that is the first offset, nothing will be truncated and `minus` and the returned
    /// value will be `0`.
    ///
    /// If `minus` is larger than all contained offsets, the log gets cleared and `minus` is capped
    /// to the actual sum of truncated offsets.
    ///
    /// `minus` will never be increased.
    ///
    /// This method assumes that it currently does not contain a future segment.
    pub(super) fn truncate_past(&mut self, minus: &mut u64) -> u64 {
        self.debug_assert_no_future();

        let mut iter = PastToNow {
            iter: self.offsets.iter_mut(),
            index: 0,
            index_max: self.meta.index,
        };
        let minus_target = core::mem::take(minus);
        let mut removed_offsets = 0;

        for item in iter.by_ref() {
            match item {
                PastToNowItem::Streak(item) => {
                    if !item.streak.one {
                        // always fully drain 0-offset streaks because they do not advance minus to
                        // minus_target which is what would bring this iteration closer to return
                        // early

                        removed_offsets += item.streak.max as u64 + 1;

                        if item.last {
                            self.clear();
                            return removed_offsets;
                        }

                        continue;
                    }

                    // 1-offset streak

                    let streak_len = item.streak.max as u64 + 1;
                    let remaining = minus_target - *minus;
                    let last = item.last;

                    if streak_len <= remaining {
                        // streak is not enough to reach minus_target, truncate it

                        *minus += streak_len;
                        removed_offsets += streak_len;

                        if last {
                            self.clear();
                            return removed_offsets;
                        }
                    } else {
                        // streak exceeds minus_target, reduce it

                        let remaining_u8 = remaining as u8;
                        *item.raw -= remaining_u8;
                        *minus += remaining;
                        removed_offsets += remaining;
                        let to_drain = iter.index - 1;
                        // todo: use truncate_front https://github.com/rust-lang/rust/issues/140667
                        self.offsets.drain(..to_drain);
                        self.meta.index -= to_drain;

                        if last {
                            // if the last item is a streak, self.meta should be Some too
                            let (streak, step) = self.meta.streak_and_step.as_mut().unwrap();
                            streak.max -= remaining_u8;
                            *step -= remaining_u8;
                        }

                        return removed_offsets;
                    }
                }
                PastToNowItem::NonStreak(item) => {
                    if item.offset > minus_target - *minus {
                        // offset leads to a frame that is > meta.log_end, keep it

                        // todo: use truncate_front https://github.com/rust-lang/rust/issues/140667
                        self.offsets.drain(..item.index);
                        self.meta.index -= item.index;

                        return removed_offsets;
                    }

                    // truncate offset in some next step

                    *minus += item.offset;
                    removed_offsets += 1;

                    if item.last {
                        self.clear();
                        return removed_offsets;
                    }
                }
            };
        }

        // reaching this means the log is empty or starts with an offset larger than minus
        debug_assert_eq!(*minus, 0);
        debug_assert_eq!(removed_offsets, 0);
        0
    }

    pub(super) fn clear(&mut self) {
        self.offsets.clear();
        self.meta = OffsetMeta::default();
    }

    /// Push an offset to the log.
    ///
    /// This method assumes that it currently does not contain a future segment.
    pub(super) fn push(&mut self, offset: u64) {
        self.debug_assert_no_future();

        #[expect(clippy::match_overlapping_arm, reason = "readability")]
        match offset {
            0 => self.push_streak::<false>(),
            1 => self.push_streak::<true>(),
            ..MULTI_BYTE_OFFSET_0 => {
                self.meta.index += 1;
                self.meta.streak_and_step = None;
                self.offsets.push_back((offset - SINGLE_BYTE_OFFSET) as u8);
            }
            ..MULTI_BYTE_OFFSET_1 => self.push_bytes::<0>(offset),
            ..MULTI_BYTE_OFFSET_2 => self.push_bytes::<1>(offset),
            ..MULTI_BYTE_OFFSET_3 => self.push_bytes::<2>(offset),
            ..MULTI_BYTE_OFFSET_4 => self.push_bytes::<3>(offset),
            ..MULTI_BYTE_OFFSET_5 => self.push_bytes::<4>(offset),
            ..MULTI_BYTE_OFFSET_6 => self.push_bytes::<5>(offset),
            ..MULTI_BYTE_OFFSET_7 => self.push_bytes::<6>(offset),
            ..MULTI_BYTE_OFFSET_8 => self.push_bytes::<7>(offset),
            _ => self.push_bytes::<8>(offset),
        }
    }

    fn push_streak<const ONE: bool>(&mut self) {
        if let Some((streak, step)) = &mut self.meta.streak_and_step
            && streak.one == ONE
            && *step < NON_WRAPPED_BYTE_MASK
        {
            streak.max += 1;
            *step += 1;
            // if self.meta is Some, self.offset is expected to not be empty
            *self.offsets.back_mut().unwrap() += 1;
        } else {
            self.meta.index += 1;
            let streak = Streak { max: 0, one: ONE };
            self.meta.streak_and_step = Some((streak, 0));
            self.offsets.push_back(streak.into());
        }
    }

    fn push_bytes<const WRAPPED: usize>(&mut self, mut offset: u64) {
        self.meta.index += WRAPPED + 2; // push 2 wrapping bytes along the WRAPPED bytes
        self.meta.streak_and_step = None; // this is no streak of 0 or 1
        self.offsets.reserve(WRAPPED + 2);
        offset -= MULTI_BYTE_OFFSETS[WRAPPED];

        let byte = (offset & NON_WRAPPED_BYTE_MASK as u64) as u8 | WRAPPING_OFFSET_OR;
        self.offsets.push_back(byte);
        offset >>= NON_WRAPPED_BYTE_MASK.trailing_ones();

        for _ in 0..WRAPPED {
            let byte = (offset & WRAPPED_OFFSET_MASK as u64) as u8;
            self.offsets.push_back(byte);
            offset >>= WRAPPED_OFFSET_MASK.trailing_ones();
        }

        self.offsets.push_back(offset as u8 | WRAPPING_OFFSET_OR);
    }

    fn debug_assert_no_future(&self) {
        debug_assert_eq!(self.meta.index, self.offsets.len());
        #[cfg(debug_assertions)]
        if let Some((streak, step)) = self.meta.streak_and_step {
            assert_eq!(streak.max, step)
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct OffsetMeta {
    index: usize,
    streak_and_step: Option<(Streak, u8)>,
}

struct PastToNow<'a> {
    iter: IterMut<'a, u8>,
    index: usize,
    index_max: usize,
}

impl<'a> Iterator for PastToNow<'a> {
    type Item = PastToNowItem<'a>;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let first_byte = self.iter.next()?;
        let index = self.index;
        self.index += 1;

        match FirstByte::from(&*first_byte) {
            FirstByte::SingleByteStreak(streak) => {
                Some(PastToNowItem::Streak(PastToNowItemStreak {
                    streak,
                    raw: first_byte,
                    last: self.index == self.index_max,
                }))
            }
            FirstByte::SingleByteOffset(offset) => {
                Some(PastToNowItem::NonStreak(PastToNowItemNonStreak {
                    offset,
                    index,
                    last: self.index == self.index_max,
                }))
            }
            FirstByte::MultiByteOffsetIncomplete(mut offset) => {
                read_bytes_forward(&mut self.iter, &mut self.index, &mut offset);
                Some(PastToNowItem::NonStreak(PastToNowItemNonStreak {
                    offset,
                    index,
                    last: self.index == self.index_max,
                }))
            }
        }
    }
}

enum PastToNowItem<'a> {
    Streak(PastToNowItemStreak<'a>),
    NonStreak(PastToNowItemNonStreak),
}

struct PastToNowItemStreak<'a> {
    streak: Streak,
    raw: &'a mut u8,
    last: bool,
}

struct PastToNowItemNonStreak {
    offset: u64,
    index: usize,
    last: bool,
}

#[derive(Debug)]
pub(super) struct NowToFuture<'a> {
    iter: Iter<'a, u8>,
    meta: OffsetMeta,
    meta_mut: &'a mut OffsetMeta,
}

impl NowToFuture<'_> {
    pub(super) fn sync(&mut self) {
        *self.meta_mut = self.meta;
    }
}

impl Iterator for NowToFuture<'_> {
    type Item = u64;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
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

        match FirstByte::from(first_byte) {
            FirstByte::SingleByteOffset(offset) => Some(offset),
            FirstByte::SingleByteStreak(streak) => {
                self.meta.streak_and_step = Some((streak, 0));
                Some(streak.one as u64)
            }
            FirstByte::MultiByteOffsetIncomplete(mut offset) => {
                read_bytes_forward(&mut self.iter, &mut self.meta.index, &mut offset);
                Some(offset)
            }
        }
    }
}

fn read_bytes_forward<B: Borrow<u8>>(
    iter: &mut impl Iterator<Item = B>,
    index: &mut usize,
    offset: &mut u64,
) {
    let mut extra_offsets_index = 0;

    // wrapping bytes contain 6 usable bits for the offset
    let mut shift = NON_WRAPPED_BYTE_MASK.trailing_ones();

    loop {
        let byte = *iter.next().unwrap().borrow(); // encoding expects more bytes to follow

        *index += 1;

        if byte.leading_zeros() == 0 {
            // this is a wrapping byte

            // the added bits are more significant
            *offset |= ((byte & NON_WRAPPED_BYTE_MASK) as u64) << shift;

            // multi-byte offsets are always at least MULTI_BYTE_OFFSET, so the encoded
            // offset is less than that to potenitally require a byte less
            *offset += MULTI_BYTE_OFFSETS[extra_offsets_index];

            return;
        }

        // this is a wrapped byte

        // the added bits are more significant, has no marker bits that need to be masked away
        *offset |= (byte as u64) << shift;

        // wrapped bytes contain 7 usable bits for the offset
        shift += WRAPPED_OFFSET_MASK.trailing_ones();

        extra_offsets_index += 1;
    }
}

#[derive(Debug)]
pub(super) struct NowToPast<'a> {
    iter: Iter<'a, u8>,
    meta: OffsetMeta,
    meta_mut: &'a mut OffsetMeta,
    streak_set_pending: bool,
}

impl NowToPast<'_> {
    pub(super) fn sync(&mut self) {
        self.set_pending_streak();
        *self.meta_mut = self.meta;
    }
    fn set_pending_streak(&mut self) {
        if self.streak_set_pending {
            self.streak_set_pending = false;
            if let Some(FirstByte::SingleByteStreak(streak)) =
                self.iter.clone().next_back().map(FirstByte::from)
            {
                self.iter.next_back();
                self.meta.streak_and_step = Some((streak, streak.max));
            }
        }
    }
}

impl Iterator for NowToPast<'_> {
    type Item = u64;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.set_pending_streak();
        if let Some((Streak { one, .. }, ref mut step)) = self.meta.streak_and_step {
            if *step == 0 {
                self.meta.streak_and_step = None;
                self.streak_set_pending = true;
                self.meta.index -= 1;
            } else {
                *step -= 1;
            }
            return Some(one as u64);
        }

        let first_byte = self.iter.next_back()?;
        self.meta.index -= 1;

        let mut offset = match FirstByte::from(first_byte) {
            FirstByte::SingleByteOffset(offset) => {
                self.streak_set_pending = true;
                return Some(offset);
            }
            FirstByte::SingleByteStreak(streak) => {
                if streak.max == 0 {
                    self.streak_set_pending = true;
                } else {
                    self.meta.streak_and_step = Some((streak, streak.max - 1));
                }
                return Some(streak.one as u64);
            }
            FirstByte::MultiByteOffsetIncomplete(offset) => offset,
        };

        self.streak_set_pending = true;
        let mut extra_offsets_index = 0;

        loop {
            let byte = *self.iter.next_back().unwrap(); // encoding expects more bytes to follow
            self.meta.index -= 1;

            if byte.leading_zeros() == 0 {
                // this is a wrapping byte

                // the added bits are less significant, wrapping bytes contain 6 usable bits for the
                // offset
                offset = (offset << NON_WRAPPED_BYTE_MASK.trailing_ones())
                    | (byte & NON_WRAPPED_BYTE_MASK) as u64;

                // multi-byte offsets are always at least MULTI_BYTE_OFFSET, so the encoded
                // offset is less than that to potenitally require a byte less
                offset += MULTI_BYTE_OFFSETS[extra_offsets_index];

                return Some(offset);
            }

            // this is a wrapped byte

            // the added bits are less significant, has no marker bits that need to be masked away,
            // wrapped bytes contain 7 usable bits for the offset
            offset = (offset << WRAPPED_OFFSET_MASK.trailing_ones()) | byte as u64;

            extra_offsets_index += 1;
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Streak {
    max: u8,
    one: bool,
}

impl From<Streak> for u8 {
    fn from(value: Streak) -> Self {
        if value.one {
            STREAK_ONE_OR | value.max
        } else {
            STREAK_ZERO_OR | value.max
        }
    }
}

enum FirstByte {
    SingleByteOffset(u64),
    SingleByteStreak(Streak),
    MultiByteOffsetIncomplete(u64),
}

impl From<&u8> for FirstByte {
    fn from(&value: &u8) -> Self {
        match value >> 6 {
            0b10 => Self::SingleByteStreak(Streak {
                max: value & NON_WRAPPED_BYTE_MASK,
                one: true
            }),
            0b01 => Self::SingleByteStreak(Streak {
                max: value & NON_WRAPPED_BYTE_MASK,
                one: false
            }),
            0b00 => Self::SingleByteOffset(value as u64 + SINGLE_BYTE_OFFSET),
            _ /*0b11*/ => Self::MultiByteOffsetIncomplete((value & NON_WRAPPED_BYTE_MASK) as u64)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use alloc::vec::Vec;

    #[rustfmt::skip]
    const OFFSETS_ENCODED: [u8; 114] = [
        // single-byte offsets

        0b01_000000, 0b10_000000, 0b00_000000, 0b00_111111,

        0b01_111111,

        0b10_111111,

        // multi-byte offsets

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
        // single-byte offsets

        0, 1, 2, 65,

        // 0 x64
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,

        // 1 x64
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,

        // multi-byte offsets

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

    #[test]
    fn now_to_past_works() {
        let mut log = OffsetLog {
            offsets: OFFSETS_ENCODED.into(),
            meta: OffsetMeta::default(),
        };

        // set to future end
        log.meta.index = OFFSETS_ENCODED.len();

        // go to past but not to the end to test another attempt with OffsetMeta tracking a streak
        let mut iter = log.now_to_past();
        let actual = iter
            .by_ref()
            .take(OFFSETS_DECODED.len() - 1)
            .collect::<Vec<_>>();
        let expected = OFFSETS_DECODED
            .into_iter()
            .skip(1)
            .rev()
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);

        iter.sync();
        assert!(log.meta.streak_and_step.is_some());

        let actual = log.now_to_past().collect::<Vec<_>>();
        assert_eq!(actual, &OFFSETS_DECODED[..1])
    }

    #[test]
    fn offset_iter_sync_works() {
        let mut log = OffsetLog {
            offsets: OFFSETS_ENCODED.into(),
            meta: OffsetMeta::default(),
        };

        // from start to some end and back
        for steps in 0..OFFSETS_DECODED.len() {
            let mut iter = log.now_to_future();
            for &expected in &OFFSETS_DECODED[..steps] {
                let actual = iter.next().unwrap();
                assert_eq!(actual, expected);
            }
            iter.sync();

            let mut iter = log.now_to_past();
            for &expected in OFFSETS_DECODED[..steps].iter().rev() {
                let actual = iter.next().unwrap();
                assert_eq!(actual, expected);
            }
            iter.sync();
        }

        // set to future end
        log.meta.index = OFFSETS_ENCODED.len();

        // from end to some start and back
        for steps in 0..OFFSETS_DECODED.len() {
            let min = OFFSETS_DECODED.len() - steps;

            let mut iter = log.now_to_past();
            for index in (0..steps).rev() {
                let actual = iter.next().unwrap();
                assert_eq!(actual, OFFSETS_DECODED[index + min],);
            }
            iter.sync();

            let mut iter = log.now_to_future();
            for index in 0..steps {
                let actual = iter.next().unwrap();
                assert_eq!(actual, OFFSETS_DECODED[index + min],);
            }
            iter.sync();
        }
    }

    #[test]
    fn truncate_future_works() {
        let mut log = OffsetLog {
            offsets: OFFSETS_ENCODED.into(),
            meta: OffsetMeta::default(),
        };

        // set to future end
        log.meta.index = OFFSETS_ENCODED.len();

        for offset in OFFSETS_DECODED.into_iter().rev() {
            let mut iter = log.now_to_past();
            assert_eq!(iter.next(), Some(offset));
            iter.sync();
            log.truncate_future();

            let mut iter = log.now_to_future();
            assert_eq!(iter.next(), None);
        }
    }

    #[test]
    fn push_works() {
        let mut log = OffsetLog::default();

        for offset in OFFSETS_DECODED {
            log.push(offset);
        }

        assert_eq!(log.offsets, OFFSETS_ENCODED);
    }

    mod truncate_out_of_log_past_works {
        use super::*;

        fn test<const N: usize>(
            offsets: [u64; N],
            mut minus: u64,
            expected_minus: u64,
            expected_removed_offsets: u64,
        ) {
            let mut log = OffsetLog::default();
            let total = offsets.iter().sum::<u64>();
            for offset in offsets.iter().cloned() {
                log.push(offset);
            }
            let total_pushed = log.now_to_past().sum::<u64>();
            assert_eq!(total, total_pushed);
            let actual_removed_offsets = log.truncate_past(&mut minus);
            let total_truncated = log.now_to_past().sum::<u64>();
            assert_eq!(total_pushed - total_truncated, expected_minus); // err
            assert_eq!(minus, expected_minus);
            assert_eq!(actual_removed_offsets, expected_removed_offsets);
            let offsets = offsets
                .into_iter()
                .enumerate()
                .skip(expected_removed_offsets as usize)
                .rev();
            let mut iter = log.now_to_past();
            for (i, offset) in offsets {
                assert_eq!(iter.next(), Some(offset), "offset #{i}");
            }
            assert_eq!(iter.next(), None);
        }

        #[test]
        fn empty_log() {
            test([], 1, 0, 0);
        }

        #[test]
        fn full_zero_streak() {
            test([0, 0, 0, 3], 1, 0, 3);
        }

        #[test]
        fn full_zero_streak_as_last() {
            test([0, 0, 0], 1, 0, 3);
        }

        #[test]
        fn none_of_one_streak() {
            test([1, 1], 0, 0, 0);
        }

        #[test]
        fn some_of_one_streak() {
            test([1, 1], 1, 1, 1);
        }

        #[test]
        fn all_of_one_streak() {
            test([1, 1, 3], 3, 2, 2);
        }

        #[test]
        fn all_of_one_streak_as_last() {
            test([1, 1], 3, 2, 2);
        }

        #[test]
        fn not_single_byte_offset() {
            test([8], 7, 0, 0);
        }

        #[test]
        fn single_byte_offset() {
            test([8, 3], 9, 8, 1);
        }

        #[test]
        fn single_byte_offset_as_last() {
            test([8], 9, 8, 1);
        }

        #[test]
        fn not_multi_byte_offset() {
            test([80], 79, 0, 0);
        }

        #[test]
        fn multi_byte_offset() {
            test([80, 3], 81, 80, 1);
        }

        #[test]
        fn multi_byte_offset_as_last() {
            test([80], 81, 80, 1);
        }
    }
}
