//! This module contains a specialized log to store frame offsets for
//! [`UpdateLog`](super::UpdateLog).
//!
//! # Motivation
//!
//! `UpdateLog` needs to keep track at which frame it has been updated. A very simple implementation
//! of this could be if it contained a `VecDeque<u64>`. However, this would mean it would grow by
//! eight bytes per update. Updates that happen almost every frame or even many times per frame
//! would consume a large amount of data very quickly.
//!
//! Instead, `UpdateLog` has an internal, lower level log, namely [`OffsetLog`].
//!
//! # Memory optimizations
//!
//! Instead of storing the frames `UpdateLog` ran, it stores the amount of frames since the last
//! update. As these offsets are in pretty much every case far smaller than a frame can become, this
//! enables multiple memory optimizations:
//!
//! ## 1. Variable length of stored integers
//!
//! Small offsets should not be stored as `u64` values as that would give no benefits. Instead,
//! they should not consume more bytes than needed to store them.
//!
//! Since the bytes also need to store some kind of flag that indicates how many bytes an offset
//! is made of, this cannot be 100% efficient. For example, an offset of `255` requires more than
//! one byte. Still, at least offsets of up to `65` do fit into a single byte, followed by `4161`
//! in two bytes, `528449` in three bytes, etc.
//!
//! ## 2. Storing multiple offsets of `1` in a single byte
//!
//! It is expected that many `UpdateLog`s still mostly are updated once every frame. So the encoding
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
//!
//! # Encoding
//!
//! - Streaks of `0` are encoded in `0b01_xxxxxx` where `x` are available for storing an integer of
//!   up to `63`. Since there is no concept of "zero times an offset of `0`", a byte of
//!   `0b01_000000` is interpreted as "one time an offset of `0`" instead.
//! - Streaks of `1` are encoded in `0b10_xxxxxx` and work otherwise in the same way as streaks of
//!   `0` do.
//! - Offsets of `2` to `65` are encoded in `0b00_xxxxxx`. Since offsets of `0` and `1` are already
//!   covered by streak encodings, a byte of `0b00_000000` is interpreted as "an offset of `2`".
//! - Offsets stored in multiple bytes are encoded in `0b11_xxxxxx` for the first and last byte and
//!   `0b0_xxxxxxx` for all bytes in between, if any are needed. As `UpdateLog` needs to be able to
//!   read the offset in reverse too, the last byte needs to contain the two flag bits too.
//! - Depending on how many in-between "wrapped" bytes there are, a constant value is added to the
//!   offset. This way, `[0b11_000000, 0b11_000000]` and `[0b11_000000, 0b0_0000000, 0b11_000000]`
//!   decode to different offsets despite the payload bits combine to the same integer. This further
//!   compresses the offset values into potentially less bytes.

use core::borrow::Borrow;
use std::collections::{
    VecDeque,
    vec_deque::{Iter, IterMut},
};

const WRAPPED_OFFSET_MASK: u8 = 0b0_1111111;
const NON_WRAPPED_BYTE_MASK: u8 = 0b00_111111;
const WRAPPING_OFFSET_OR: u8 = 0b11_000000;
const STREAK_ZERO_OR: u8 = 0b01_000000;
const STREAK_ONE_OR: u8 = 0b10_000000;
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

#[derive(Debug, Default)]
pub(super) struct OffsetLog {
    offsets: VecDeque<u8>,
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

    fn past_to_now(&mut self) -> PastToNow {
        debug_assert_eq!(self.meta.index, self.offsets.len());
        PastToNow {
            iter: self.offsets.iter_mut(),
            index: 0,
            index_max: self.meta.index,
        }
    }

    /// Returns an iterator that yields the decoded offsets as `u64`, going from the chronologically
    /// next offsets further into the future.
    ///
    /// Use [`NowToFuture::sync`] to synchronize the log position with the iterator.
    pub(super) fn now_to_future(&mut self) -> NowToFuture {
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
    pub(super) fn now_to_past(&mut self) -> NowToPast {
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
        if let Some((ref mut streak, step)) = self.meta.streak_and_step {
            if streak.max != step {
                streak.max = step;
                *self.offsets.back_mut().unwrap() = (*streak).into();
            }
        }
    }

    /// Truncates all past offsets that sum up to `minus`.
    ///
    /// The next offset that exceeds `minus` will be replaced with an offset of `0`. Its former
    /// value will also be added to `minus`.
    ///
    /// If `minus` is larger than the total sum of offsets in the log, it will be reduced to the
    /// actual sum. In this case the log is also fully cleared.
    ///
    /// This method returns the amount of offsets that were truncated.
    ///
    /// This method assumes that it currently does not contain a future segment.
    pub(super) fn truncate_out_of_log(&mut self, minus: &mut u64) -> u64 {
        if self.meta.index == 0 {
            *minus = 0;
            return 0;
        }

        let minus_target = core::mem::take(minus);
        let mut iter = self.past_to_now();
        let mut removed_offsets = 0;

        for item in iter.by_ref() {
            match item {
                PastToNowItem::Streak(item) if !item.streak.one => {
                    // zero-offset fully in the past
                    removed_offsets += item.streak.max as u64 + 1;
                    if item.last {
                        self.clear();
                        return removed_offsets;
                    }
                }
                PastToNowItem::Streak(item) => {
                    let max64 = item.streak.max as u64;
                    let streak_len = max64 + 1;
                    let remaining = minus_target - *minus;
                    match max64.checked_sub(remaining) {
                        None => {
                            // fully remove the byte
                            *minus += streak_len;
                            removed_offsets += streak_len;
                            if item.last {
                                self.clear();
                                return removed_offsets;
                            }
                        }
                        Some(0) => {
                            // streak is reduced to 0 which translates to one 1-offset
                            // replace it with one 0-offset
                            *minus += streak_len;
                            removed_offsets += max64;
                            let patch = iter.streak_patch(item);
                            patch(self);
                            return removed_offsets;
                        }
                        Some(_) => {
                            let last = item.last;
                            let remaining_plus_one = remaining as u8 + 1;
                            *item.raw -= remaining_plus_one;
                            *minus += remaining_plus_one as u64;
                            removed_offsets += remaining;
                            let to_drain = iter.index - 1;
                            self.offsets.drain(..to_drain);
                            self.offsets.push_front(STREAK_ZERO_OR);
                            self.meta.index -= to_drain;
                            self.meta.index += 1;
                            if last {
                                let (streak, step) = self.meta.streak_and_step.as_mut().unwrap();
                                streak.max -= remaining_plus_one;
                                *step -= remaining_plus_one;
                            }
                            return removed_offsets;
                        }
                    }
                }
                PastToNowItem::NonStreak(item) => {
                    *minus += item.offset;
                    if *minus > minus_target {
                        let patch = iter.non_streak_patch(item);
                        patch(self);
                        return removed_offsets;
                    }
                    removed_offsets += 1;
                    if item.last {
                        self.clear();
                        return removed_offsets;
                    }
                }
            };
        }
        let to_drain = iter.index;
        match iter.next().unwrap() {
            PastToNowItem::Streak(PastToNowItemStreak {
                streak: Streak { one: false, .. },
                ..
            }) => {
                self.offsets.drain(..to_drain);
                self.meta.index -= to_drain;
            }
            PastToNowItem::Streak(item) if item.streak.max == 0 => {
                *minus += 1;
                let patch = iter.streak_patch(item);
                patch(self);
            }
            PastToNowItem::Streak(item) => {
                let last = item.last;
                *item.raw -= 1;
                *minus += 1;
                self.offsets.drain(..to_drain);
                self.offsets.push_front(STREAK_ZERO_OR);
                self.meta.index -= to_drain;
                self.meta.index += 1;
                if last {
                    self.set_meta_after_first_offset();
                }
            }
            PastToNowItem::NonStreak(item) => {
                *minus += item.offset;
                let patch = iter.non_streak_patch(item);
                patch(self);
            }
        }
        removed_offsets
    }
    pub(super) fn clear(&mut self) {
        self.offsets.clear();
        self.meta = OffsetMeta::default();
    }
    /// Expects [`Self::truncate_future`] to be called in advance.
    ///
    /// Returns `true` if the log was empty before the call.
    pub(super) fn push_was_empty(&mut self, offset: u64) -> bool {
        match offset {
            _ if self.offsets.is_empty() => {
                // first offset is always a zero
                self.meta.index = 1;
                let streak = Streak { max: 0, one: false };
                self.meta.streak_and_step = Some((streak, 0));
                self.offsets.push_back(streak.into());
                return true;
            }
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
        false
    }
    fn push_streak<const ONE: bool>(&mut self) {
        if let Some((streak, step)) = &mut self.meta.streak_and_step
            && streak.one == ONE
            && *step < NON_WRAPPED_BYTE_MASK
        {
            streak.max += 1;
            *step += 1;
            *self.offsets.back_mut().unwrap() += 1;
        } else {
            self.meta.index += 1;
            let streak = Streak { max: 0, one: ONE };
            self.meta.streak_and_step = Some((streak, 0));
            self.offsets.push_back(streak.into());
        }
    }
    fn push_bytes<const WRAPPED: usize>(&mut self, mut offset: u64) {
        self.meta.index += WRAPPED + 2;
        self.meta.streak_and_step = None;
        self.offsets.reserve(WRAPPED + 2);
        offset -= MULTI_BYTE_OFFSETS[WRAPPED];

        let byte = (offset & NON_WRAPPED_BYTE_MASK as u64) as u8 | WRAPPING_OFFSET_OR;
        self.offsets.push_back(byte);
        offset >>= 6;

        for _ in 0..WRAPPED {
            let byte = (offset & WRAPPED_OFFSET_MASK as u64) as u8;
            self.offsets.push_back(byte);
            offset >>= 7;
        }

        self.offsets.push_back(offset as u8 | WRAPPING_OFFSET_OR);
    }
    fn set_meta_after_first_offset(&mut self) {
        debug_assert_eq!(self.meta.index, 1);
        debug_assert_eq!(self.offsets.len(), 1);
        debug_assert_eq!(self.offsets[0] & WRAPPING_OFFSET_OR, STREAK_ZERO_OR);
        self.meta.streak_and_step = Some((Streak { max: 0, one: false }, 0));
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

impl PastToNow<'_> {
    /// Returns a closure that either replaces `item` in [`OffsetLog::offsets`] with an `0` offset
    /// or, which is preferred, removes it and increases an immediately following, existing
    /// `0`-offset streak.
    fn streak_patch(mut self, item: PastToNowItemStreak) -> impl FnOnce(&mut OffsetLog) + use<> {
        let item_index = self.index;
        let mut last = item.last;
        let mut increase_existing = false;
        if last {
            *item.raw = STREAK_ZERO_OR;
        } else {
            while let Some(PastToNowItem::Streak(PastToNowItemStreak {
                streak: Streak { max, one: false },
                raw,
                last: next_last,
            })) = self.next()
            {
                if max == NON_WRAPPED_BYTE_MASK {
                    continue;
                }
                *raw += 1;
                last = next_last;
                increase_existing = true;
                break;
            }
            if !increase_existing {
                *item.raw = STREAK_ZERO_OR;
            }
        }
        move |offset_log| {
            if increase_existing {
                let to_drain = item_index;
                offset_log.meta.index -= to_drain;
                offset_log.offsets.drain(..to_drain);
                if last {
                    let (streak, step) = offset_log.meta.streak_and_step.as_mut().unwrap();
                    streak.max += 1;
                    *step += 1;
                }
            } else {
                let to_drain = item_index - 1;
                offset_log.offsets.drain(..to_drain);
                offset_log.meta.index -= to_drain;
                if last {
                    offset_log.set_meta_after_first_offset();
                }
            }
        }
    }

    /// Returns a closure that either replaces `item` in [`OffsetLog::offsets`] with an `0` offset
    /// or, which is preferred, removes it and increases an immediately following, existing
    /// `0`-offset streak.
    fn non_streak_patch(
        mut self,
        item: PastToNowItemNonStreak,
    ) -> impl FnOnce(&mut OffsetLog) + use<> {
        let item_index = self.index;
        let mut last = item.last;
        let mut increase_existing = false;
        let single_byte_offset = item.raw.is_some();
        if last {
            if let Some(raw) = item.raw {
                *raw = STREAK_ZERO_OR;
            }
        } else {
            while let Some(PastToNowItem::Streak(PastToNowItemStreak {
                streak: Streak { max, one: false },
                raw,
                last: next_last,
            })) = self.next()
            {
                if max == NON_WRAPPED_BYTE_MASK {
                    continue;
                }
                *raw += 1;
                last = next_last;
                increase_existing = true;
                break;
            }
            if !increase_existing {
                if let Some(raw) = item.raw {
                    *raw = STREAK_ZERO_OR;
                }
            }
        }

        move |offset_log| {
            if increase_existing {
                let to_drain = item_index;
                offset_log.meta.index -= to_drain;
                offset_log.offsets.drain(..to_drain);
                if last {
                    let (streak, step) = offset_log.meta.streak_and_step.as_mut().unwrap();
                    streak.max += 1;
                    *step += 1;
                }
                return;
            }
            if single_byte_offset {
                let to_drain = item_index - 1;
                offset_log.offsets.drain(..to_drain);
                offset_log.meta.index -= to_drain;
            } else {
                let to_drain = item_index;
                offset_log.offsets.drain(..to_drain);
                offset_log.offsets.push_front(STREAK_ZERO_OR);
                offset_log.meta.index -= to_drain;
                offset_log.meta.index += 1;
            }
            if last {
                offset_log.set_meta_after_first_offset();
            }
        }
    }
}

impl<'a> Iterator for PastToNow<'a> {
    type Item = PastToNowItem<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let first_byte = self.iter.next()?;
        self.index += 1;

        match FirstByte::from(&*first_byte) {
            FirstByte::SingleByteOffset(offset) => {
                Some(PastToNowItem::NonStreak(PastToNowItemNonStreak {
                    offset,
                    raw: Some(first_byte),
                    last: self.index == self.index_max,
                }))
            }
            FirstByte::SingleByteStreak(streak) => {
                Some(PastToNowItem::Streak(PastToNowItemStreak {
                    streak,
                    raw: first_byte,
                    last: self.index == self.index_max,
                }))
            }
            FirstByte::MultiByteOffsetIncomplete(mut offset) => {
                read_bytes_forward(&mut self.iter, &mut self.index, &mut offset);
                Some(PastToNowItem::NonStreak(PastToNowItemNonStreak {
                    offset,
                    raw: None,
                    last: self.index == self.index_max,
                }))
            }
        }
    }
}

enum PastToNowItem<'a> {
    Streak(PastToNowItemStreak<'a>),
    NonStreak(PastToNowItemNonStreak<'a>),
}

struct PastToNowItemStreak<'a> {
    streak: Streak,
    raw: &'a mut u8,
    last: bool,
}

struct PastToNowItemNonStreak<'a> {
    offset: u64,
    raw: Option<&'a mut u8>,
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
    let mut shift = 6;

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
        shift += 7;

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

impl TryFrom<&u8> for Streak {
    type Error = ();
    fn try_from(value: &u8) -> Result<Self, Self::Error> {
        let value = *value;
        match value >> 6 {
            0b01 => Ok(Streak {
                max: value & NON_WRAPPED_BYTE_MASK,
                one: false,
            }),
            0b10 => Ok(Streak {
                max: value & NON_WRAPPED_BYTE_MASK,
                one: true,
            }),
            _ => Err(()),
        }
    }
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
            0b00 => Self::SingleByteOffset(value as u64 + SINGLE_BYTE_OFFSET), // 0 and 1 are encoded in streaks, so + 2 here
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
        0b01_000000, 0b10_000000, 0b00_000000, 0b00_111111,

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
        0, 1, 2, 65,

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

        66,
        4161,

        4162,
        528449,

        528450,
        67637313,

        67637314,
        8657571905,

        8657571906,
        1108169199681,

        1108169199682,
        141845657555009,

        141845657555010,
        18156244167036993,

        18156244167036994,
        2323999253380730945,

        2323999253380730946,
        u64::MAX
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
        log.meta.index = OFFSETS_ENCODED.len();

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
        for steps in 0..OFFSETS_DECODED.len() {
            let mut iter = log.now_to_future();
            for index in 0..steps {
                let actual = iter.next().unwrap();
                assert_eq!(actual, OFFSETS_DECODED[index],);
            }
            iter.sync();

            let mut iter = log.now_to_past();
            for index in (0..steps).rev() {
                let actual = iter.next().unwrap();
                assert_eq!(actual, OFFSETS_DECODED[index],);
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
        let mut first = true;

        for offset in OFFSETS_DECODED {
            assert_eq!(log.push_was_empty(offset), first);
            first = false;
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
        ) -> Option<u8> {
            if let Some(&first) = offsets.get(0) {
                assert_eq!(first, 0);
            }
            let mut log = OffsetLog::default();
            let total = offsets.iter().sum::<u64>();
            for offset in offsets.iter().cloned() {
                log.push_was_empty(offset);
            }
            let total_pushed = log.now_to_past().sum::<u64>();
            assert_eq!(total, total_pushed);
            let actual_removed_offsets = log.truncate_out_of_log(&mut minus);
            let total_truncated = log.now_to_past().sum::<u64>();
            assert_eq!(expected_minus, total_pushed - total_truncated);
            assert_eq!(minus, expected_minus);
            assert_eq!(actual_removed_offsets, expected_removed_offsets);
            let offsets = offsets
                .into_iter()
                .enumerate()
                .skip(expected_removed_offsets as usize)
                .rev();
            let mut iter = log.now_to_past();
            for (i, offset) in offsets {
                if i == expected_removed_offsets as usize {
                    assert_eq!(iter.next(), Some(0), "offset #{i}");
                } else {
                    assert_eq!(iter.next(), Some(offset), "offset #{i}");
                }
            }
            assert_eq!(iter.next(), None);
            log.offsets.pop_front()
        }

        #[test]
        fn empty_log() {
            test([], 1, 0, 0);
        }

        const INCREASED_FRONT: Option<u8> = Some(STREAK_ZERO_OR + 1);

        mod last_removed_zero {
            use super::*;

            mod single {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0], 1, 0, 1);
                }

                #[test]
                fn remaining_one_single() {
                    test([0, 1], 0, 1, 1);
                }

                #[test]
                fn remaining_one_streak() {
                    test([0, 1, 1], 0, 1, 1);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 10], 0, 10, 1);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 90], 0, 90, 1);
                }
            }

            mod streak {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 0], 1, 0, 2);
                }

                #[test]
                fn remaining_one_single() {
                    test([0, 0, 1], 0, 1, 2);
                }

                #[test]
                fn remaining_one_streak() {
                    test([0, 0, 1, 1], 0, 1, 2);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 0, 10], 0, 10, 2);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 0, 90], 0, 90, 2);
                }
            }
        }

        mod last_removed_one_streak {
            use super::*;

            mod almost_none {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 1, 1, 1], 1, 2, 2);
                }

                #[test]
                fn remaining_one_single() {
                    let mut offsets = [1; 66]; // [0, 64*1, 1*1]
                    offsets[0] = 0;
                    test(offsets, 1, 2, 2);
                }

                #[test]
                fn remaining_one_streak() {
                    let mut offsets = [1; 67]; // [0, 64*1, 2*1]
                    offsets[0] = 0;
                    test(offsets, 1, 2, 2);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 1, 1, 1, 10], 1, 2, 2);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 1, 1, 1, 90], 1, 2, 2);
                }
            }

            mod some {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 1, 1, 1], 2, 3, 3);
                }

                #[test]
                fn remaining_one_single() {
                    let mut offsets = [1; 66]; // [0, 64*1, 1*1]
                    offsets[0] = 0;
                    test(offsets, 2, 3, 3);
                }

                #[test]
                fn remaining_one_streak() {
                    let mut offsets = [1; 67]; // [0, 64*1, 2*1]
                    offsets[0] = 0;
                    test(offsets, 2, 3, 3);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 1, 1, 1, 10], 2, 3, 3);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 1, 1, 1, 90], 2, 3, 3);
                }
            }

            mod all {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 1, 1, 1], 3, 3, 4);
                }

                #[test]
                fn remaining_one_single() {
                    let mut offsets = [1; 66]; // [0, 64*1, 1*1]
                    offsets[0] = 0;
                    test(offsets, 64, 65, 65);
                }

                #[test]
                fn remaining_one_streak() {
                    let mut offsets = [1; 67]; // [0, 64*1, 2*1]
                    offsets[0] = 0;
                    test(offsets, 64, 65, 65);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 1, 1, 1, 10], 3, 13, 4);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 1, 1, 1, 90], 3, 93, 4);
                }

                #[test]
                fn remaining_zero_and_offset() {
                    let front = test([0, 1, 1, 1, 0, 5], 2, 3, 3);
                    assert_eq!(front, INCREASED_FRONT);
                }

                #[test]
                fn remaining_offset_and_zero() {
                    let front = test([0, 1, 1, 1, 5, 0], 3, 8, 4);
                    assert_eq!(front, INCREASED_FRONT);
                }
            }
        }

        mod last_removed_single_byte_offset {
            use super::*;

            mod partial {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 10], 5, 10, 1);
                }

                #[test]
                fn remaining_one_single() {
                    test([0, 10, 1], 5, 10, 1);
                }

                #[test]
                fn remaining_one_streak() {
                    test([0, 10, 1, 1], 5, 10, 1);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 10, 20], 5, 10, 1);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 10, 80], 5, 10, 1);
                }
            }

            mod full {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 10], 10, 10, 2);
                }

                #[test]
                fn remaining_one_single() {
                    test([0, 10, 1], 10, 11, 2);
                }

                #[test]
                fn remaining_one_streak() {
                    test([0, 10, 1, 1], 10, 11, 2);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 10, 20], 10, 30, 2);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 10, 80], 10, 90, 2);
                }

                #[test]
                fn remaining_zero_and_offset() {
                    let front = test([0, 3, 0, 5], 2, 3, 1);
                    assert_eq!(front, INCREASED_FRONT);
                }

                #[test]
                fn remaining_offset_and_zero() {
                    let front = test([0, 3, 5, 0], 3, 8, 2);
                    assert_eq!(front, INCREASED_FRONT);
                }
            }
        }

        mod last_removed_multi_byte_offset {
            use super::*;

            mod partial {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 80], 40, 80, 1);
                }

                #[test]
                fn remaining_one_single() {
                    test([0, 80, 1], 40, 80, 1);
                }

                #[test]
                fn remaining_one_streak() {
                    test([0, 80, 1, 1], 40, 80, 1);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 80, 10], 40, 80, 1);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 80, 90], 40, 80, 1);
                }
            }

            mod full {
                use super::*;

                #[test]
                fn no_remaining() {
                    test([0, 80], 80, 80, 2);
                }

                #[test]
                fn remaining_one_single() {
                    test([0, 80, 1], 80, 81, 2);
                }

                #[test]
                fn remaining_one_streak() {
                    test([0, 80, 1, 1], 80, 81, 2);
                }

                #[test]
                fn remaining_single_byte_offset() {
                    test([0, 80, 10], 80, 90, 2);
                }

                #[test]
                fn remaining_multi_byte_offset() {
                    test([0, 80, 90], 80, 170, 2);
                }

                #[test]
                fn remaining_zero_and_offset() {
                    let front = test([0, 70, 0, 5], 2, 70, 1);
                    assert_eq!(front, INCREASED_FRONT);
                }

                #[test]
                fn remaining_offset_and_zero() {
                    let front = test([0, 70, 5, 0], 70, 75, 2);
                    assert_eq!(front, INCREASED_FRONT);
                }
            }
        }
    }
}
