use crate::{log::PreUpdateVariant, meta::RevMeta};
use core::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::{TryReserveError, VecDeque, vec_deque::Iter},
    num::NonZeroU64,
    ops::ControlFlow,
};

pub(crate) mod limits;

use limits::*;

const MAX_ZEROES_PER_BYTE: u8 = 65;
const MAX_ZEROES_AS_BYTE: u8 = 0b10_111111;
const ZEROES_MASK: u8 = 0b00_111111;
const ZEROES_OR: u8 = 0b10_000000;
const MAX_SINGLE_BYTE_OFFSET: u8 = 0b0_1111111;
const WRAPPED_OFFSET_MASK: u8 = 0b0_1111111;
const WRAPPING_OFFSET_MASK: u8 = 0b00_111111;
const WRAPPING_OFFSET_OR: u8 = 0b11_000000;
const MAX_WRAPPING_OFFSET: u8 = 0b00_111111;

/// A log that keeps track when it was updated and provides an alternative value to
/// [`RevMeta::past_len`] for when these updates do not happen exactly once per [`RevUpdate`].
///
/// It is generally adviced to not update this from multiple systems as debugging becomes easier
/// if instead each system has its own `PastLenLog`.
///
/// # Examples
///
/// This log is usually used next to other [logs] that need a `max_past_len` value to determine
/// when they can shorten their log entries once these go out of log. These logs do _not_ need
/// a `PastLenLog` if they get updated exactly once every frame:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy::ecs::error::Result;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseStateLog;
/// # #[derive(Default)]
/// # struct MyType;
/// # impl MyType { fn new() -> Self { Self }}
/// # let mut app = App::new();
/// app.rev_add_systems(RevUpdate, my_system); // runs once per reversible frame
///
/// // needs no PastLenLog
/// fn my_system(meta: Res<RevMeta>, mut value_log: Local<DenseStateLog<MyType>>) -> Result {
///     match meta.running_direction() {
///         RevDirection::Forward { log } => {
///             if log {
///                 value_log.forward_log()?;
///             } else {
///                 value_log.push_and_pop_past(meta.past_len() as usize, MyType::new());
///             }
///             // deref value_log to use value
///         }
///         RevDirection::BackwardLog => {
///             // deref value_log to use value
///             value_log.backward_log()?;
///         }
///     }
///     Ok(())
/// }
/// ```
///
/// However, when you intend to update your log less often than that and `MyType` is large enough
/// that you are concerned with the size if you let it allow to grow up to [`RevMeta::past_len`],
/// you might want to use a `Sparse*` log instead (see the [logs] module) or add an `PastLenLog`
/// next to your log.
///
/// If you want to update your log more often than once per frame or generally in anarbitrary
/// manner, you might want to use a `PastLenLog` next to it:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy::ecs::error::Result;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseTransitionLog;
/// # use bevy_oozlum::log::PastLenLog;
/// # #[derive(BufferedEvent, Clone)]
/// # struct MyEvent;
/// fn my_system(
///     meta: Res<RevMeta>,
///     mut events: EventReader<MyEvent>,
///     mut past_len_log: Local<PastLenLog>,
///     mut events_log: Local<DenseTransitionLog<MyEvent>>,
/// ) -> Result {
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             // always truncate the future of the logs in case there are no events
///             past_len_log.truncate_future(&meta)?;
///             events_log.drain_future();
///
///             // there may be 0, 1 or more events per system run
///             for my_event in events.read() {
///                 // use event
///
///                 // past_len contains just the right value that events_log is shortened the
///                 // minimum amount of events to go back and forth to any point of the global
///                 // log and not a single event more
///                 let past_len = past_len_log.update_and_get_past_len(&meta)?;
///                 events_log.push_and_drain_past(past_len, my_event.clone());
///             }
///         }
///         RevDirection::FORWARD_LOG => {
///             while past_len_log.forward_log(&meta)? {
///                 let my_event = events_log.forward_log()?;
///                 // use event
///             }
///         }
///         RevDirection::BackwardLog => {
///             while past_len_log.backward_log(&meta)? {
///                 let my_event = events_log.backward_log()?;
///                 // use event
///             }
///         }
///     }
///     Ok(())
/// }
/// ```
///
/// [logs]: crate::log
/// [`RevUpdate`]: crate::schedule::RevUpdate
#[derive(Clone, Default)]
pub struct PastLenLog {
    /// Contains the offsets between the [frames](RevMeta::now) this log was updated at.
    ///
    /// The offsets are encoded in a special way to minimize the used memory as typically the
    /// use-cases have the task to keep the memory overhead low.
    ///
    /// - Offset from `0` to `127` are encoded in a single byte as `x` bits in the pattern of
    ///   `0b0_xxxxxxx`.
    /// - Up to `65` sequential offsets of `0` are encoded in a single byte as `x` bits in the
    ///   pattern of `0b10_xxxxxx`. The numeric value of the `x` is actually read plus 2. This is
    ///   because:
    ///   - There is no concept of "zero times an offset of `0`" so `0b10_000000` makes no sense to
    ///     be interpreted as "zero times".
    ///   - The value of "one time an offset of `0`" is already encoded in `0b0_0000000`.
    /// - Offsets larger than `127` are encoded in multiple bytes and are split in chunks of `x`
    ///   bits:
    ///   - The first and last byte of this sequence use the pattern `0b11_xxxxxx`.
    ///   - If more bits are needed, in between are bytes that use the pattern `0b0_xxxxxxx`.
    ///   - This uses up to ten bytes in total for `u64::MAX`
    /// - These bytes or sequences of bytes can be read in reverse as well, which is needed for
    ///   reading the previous offset in [`Self::backward_log`].
    /// - The [`OffsetIter`] iterator is used to read the offsets. See [`IterItem`].
    offset_bytes: VecDeque<u8>,

    /// A frame this log matched at (or 0) that is either at [`RevMeta::past_end`] or as closely
    /// before it as possible to determine how much other logs that run along this can be reduced
    /// to.
    ///
    /// This frame must not be a more recent frame because then [`Self::backward_log`] will be
    /// unable to match that frame as [`Self::index`] cannot be reduced further. Otherwise, the
    /// [`OutOfLog`] error is returned which is usually not encountered with this log.
    out_of_or_past_end_log: u64,

    /// The chronological last frame this log got updated
    last_update: u64,

    /// The current index into [`Self::offset_bytes`]. Always points to the first byte of an
    /// offset sequence or, when at the end of the log, is equal to the `len` of `offset_bytes`.
    index: usize,

    /// The length of the log which is what this log is keeping track of.
    past_len: u64,

    /// The current amount of sequential offsets of `0`.
    zeroes: u8,

    /// The amount of sequential offsets of `0` at the future end of the log.
    zeroes_max: u8,

    update_state: Option<PastLenState>,
}

impl Debug for PastLenLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PastLenLog")
            .field("offset_bytes", &self.offset_bytes)
            .field("offsets (decoded)", &OffsetIter(self.offset_bytes.iter()))
            .field("out_of_or_past_end_log", &self.out_of_or_past_end_log)
            .field("last_update", &self.last_update)
            .field("index", &self.index)
            .field("past_len", &self.past_len)
            .field("zeroes", &self.zeroes)
            .field("zeroes_max", &self.zeroes_max)
            .field("update_state", &self.update_state)
            .finish()
    }
}

/// Iterator to read [`PastLenLog::offset_bytes`] and decode them to [`IterItem`].
#[derive(Clone)]
struct OffsetIter<'a>(Iter<'a, u8>);

/// Reads a byte or sequence of bytes from [`PastLenLog::offset_bytes`]. Contains a single offset
/// or, if the offset is `0`, a sequence of such offsets.
#[derive(Debug, PartialEq, Clone)]
struct IterItem {
    /// Amount of frames between two updates of [`PastLenLog`].
    offset: u64,

    /// The amount of bytes this offset is made of to update [`PastLenLog::index`] correctly.
    /// If [Self::offset] == `0`, this is the amount of `0` offsets in this byte instead.
    len: NonZeroU64,
}

impl Debug for OffsetIter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
            len: NonZeroU64::MIN,
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
                len: unsafe { NonZeroU64::new_unchecked(zeroes as u64) },
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
                    len: unsafe { NonZeroU64::new_unchecked(len) },
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
        let max = len.saturating_mul(MAX_ZEROES_PER_BYTE as usize);

        (min, Some(max))
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
                    len: unsafe { NonZeroU64::new_unchecked(len) },
                });
            }

            // this is a wrapped byte

            // the added bits are less significant, has no marker bits that need to be masked away,
            // wrapped bytes contain 7 usable bits for the offset
            offset = (offset << 7) | byte as u64;
        }
    }
}

macro_rules! bytes_len_disclaimer {
    () => {
        "\nNote that the number of bytes have no relation to the length of the log."
    };
}

impl PastLenLog {
    /// Creates an empty log.
    pub const fn new() -> Self {
        Self {
            offset_bytes: VecDeque::new(),
            out_of_or_past_end_log: 0, // the minimum frame RevUpdate can go forward at is 1
            last_update: 0,
            index: 0,
            past_len: 0,
            zeroes: 0,
            zeroes_max: 0,
            update_state: None,
        }
    }

    /// Creates an empty log with space for at least `bytes_capacity` bytes.
    ///
    /// See [`VecDeque::with_capacity`].
    #[doc = bytes_len_disclaimer!()]
    pub fn with_capacity(bytes_capacity: usize) -> Self {
        Self {
            offset_bytes: VecDeque::with_capacity(bytes_capacity),
            ..Self::new()
        }
    }

    /// Returns the number of bytes in the log.
    ///
    /// See [`VecDeque::len`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_len(&self) -> usize {
        self.offset_bytes.len()
    }

    /// Returns the number of bytes the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_capacity(&self) -> usize {
        self.offset_bytes.capacity()
    }

    /// Returns `true` if the log contains no bytes.
    ///
    /// See [`VecDeque::is_empty`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_is_empty(&self) -> bool {
        self.offset_bytes.is_empty()
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_reserve(&mut self, additional: usize) {
        self.offset_bytes.reserve(additional)
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve_exact`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_reserve_exact(&mut self, additional: usize) {
        self.offset_bytes.reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve_exact`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_shrink_to(&mut self, min_capacity: usize) {
        self.offset_bytes.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    #[doc = bytes_len_disclaimer!()]
    pub fn bytes_shrink_to_fit(&mut self) {
        self.offset_bytes.shrink_to_fit()
    }

    /// Update the log which does the following:
    /// - [`Self::truncate_future`] and returns its error if one or more [`Self::backward_log`]/
    ///   [`Self::forward_log`] calls were missed. See [`PastLenNotLogError`]
    /// - removes log entries before [`RevMeta::past_len`].
    /// - returns the updated length of the log as an alternative to [`RevMeta::past_len`].
    ///
    /// This is used during [`RevDirection::NOT_LOG`] when the current scope has been determined for
    /// some operation to happen, most often in combination with another log that is updated next
    /// with the returned value.
    ///
    /// When instead the current scope has been determined for such operations to **not** happen,
    /// [`Self::truncate_future`] **must** be called instead, next to the `drain_future` method of
    /// the accompanied log(s).
    ///
    /// Note that, if the other log is a `Transition` log and the iterator returned by its
    /// `drain_future` method is used for cleanups, the value returned here should be increased by
    /// `1` if the cleanups regard things that need to be out of log first. See the [module docs]
    /// for a more detailed explaination.
    ///
    /// See the [type docs] for an example of using this method.
    ///
    /// [`RevDirection::NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
    /// [missed log traversal updates]: MissedUpdate
    /// [module docs]: super
    /// [type docs]: PastLenLog
    #[track_caller]
    pub fn update_and_get_past_len(&mut self, meta: &RevMeta) -> u64 {
        assert_eq!(self.index, self.offset_bytes.len(), "todo");

        let iter = OffsetIter(self.offset_bytes.iter());

        let mut to_drain = 0;

        for IterItem { offset, len } in iter {
            if offset == 0 {
                // Offsets of 0 are encoded differently, len is not the amount of bytes,
                // which is actually always 1 here, but the amount of zero offsets in this
                // one byte.
                // We want to get rid of these offsets as well as they dont bring
                // self.out_of_or_past_end_log any closer the limit.
                to_drain += 1;
                self.past_len -= len.get();
                continue;
            }

            let next_oldest = self.out_of_or_past_end_log + offset;
            if next_oldest > meta.past_end() {
                // next_oldest is reachable by log traversion, which is undesired because
                // Self::backward_log stops working there
                break;
            }

            to_drain += len.get() as usize;
            self.out_of_or_past_end_log = next_oldest;
            self.past_len -= 1;
        }

        self.index -= to_drain;
        // todo: use truncate_front when https://github.com/rust-lang/rust/issues/140667 stabilizes
        self.offset_bytes.drain(..to_drain);

        // push present offset
        meta.past_len_limits().push_past_len_update(
            self.update_state.expect("todo"),
            PastLenLimit::not_log_limit(meta.now()),
        );
        let mut offset = meta.now() - self.last_update;
        self.last_update = meta.now();
        self.past_len += 1;

        if offset == 0 {
            // offsets of zero are not pushed right away unless the maximum is reached

            if self.zeroes == MAX_ZEROES_PER_BYTE {
                // reached the maximum amount of zeroes that fit into a single byte, push it and
                // start a new sequence of zero offsets
                self.offset_bytes.push_back(MAX_ZEROES_AS_BYTE);
                self.index += 1;
                self.zeroes = 1;
            } else {
                // increase the sequence of zero offsets
                self.zeroes += 1;
            }

            self.zeroes_max = self.zeroes;
            return self.past_len;
        } else if self.zeroes == 1 {
            // there was an offset of 0 previously, push it now that it is sure no more such offsets
            // are following it
            self.offset_bytes.push_back(0);
            self.index += 1;
        } else if self.zeroes > 1 {
            // there was a sequence of offsets of 0 previously, push it now that it is sure no more
            // such offsets are following it
            self.offset_bytes.push_back((self.zeroes - 2) | ZEROES_OR);
            self.index += 1;
        }

        self.index += 1;
        self.zeroes = 0;
        self.zeroes_max = 0;

        if offset <= MAX_SINGLE_BYTE_OFFSET as u64 {
            self.offset_bytes.push_back(offset as u8);
            return self.past_len;
        }

        // this is a multi-byte offset

        let wrapping_byte = (offset & WRAPPING_OFFSET_MASK as u64) as u8 | WRAPPING_OFFSET_OR;
        self.offset_bytes.push_back(wrapping_byte);

        // wrapping bytes contain 6 usable bits for the offset
        offset >>= 6;

        loop {
            self.index += 1;
            if offset <= MAX_WRAPPING_OFFSET as u64 {
                // this is a wrapping byte

                self.offset_bytes
                    .push_back(offset as u8 | WRAPPING_OFFSET_OR);
                return self.past_len;
            }

            // this is a wrapped byte

            self.offset_bytes
                .push_back((offset & WRAPPED_OFFSET_MASK as u64) as u8);

            // wrapped bytes contain 7 usable bits for the offset
            offset >>= 7;
        }
    }

    /// Checks at [`RevDirection::BackwardLog`] if this log has been updated at this frame.
    ///
    /// Returns `Ok(true)` if that is the case or `Ok(false)` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `Ok(false)` then as well.
    ///
    /// If this returns an error, an update has been missed, the log has been constructed from an
    /// invalid state or [`RevMeta`] is in an invalid state. See [`PastLenBackwardError`].
    ///
    /// If this log is potenitally updated more than once per frame, use this method in the fitting
    /// amount of `if` cases or with a `while` loop.
    ///
    /// See the [type docs] for an example.
    ///
    /// [`RevDirection::BackwardLog`]: crate::meta::RevDirection::BackwardLog
    /// [type docs]: PastLenLog
    #[track_caller]
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        let now = meta.now() + 1;
        // set next backward_limit if this method returns true
        let backward_limit = match self.last_update.cmp(&now) {
            // did not yet reach the next past frame in the log, may be at end of reachable log
            Ordering::Less => return false,
            Ordering::Equal => {
                let mut iter = OffsetIter(self.offset_bytes.range(..self.index));
                match iter.next_back() {
                    Some(IterItem { offset: 0, len }) => {
                        self.index -= 1;
                        self.past_len -= 1;
                        self.zeroes = len.get() as u8 - 1;
                        if self.zeroes == 0 {
                            match iter.next_back() {
                                Some(IterItem { offset, .. }) => now - offset,
                                None => u64::MIN,
                            }
                        } else {
                            now
                        }
                    }
                    Some(IterItem { offset, len }) => {
                        self.last_update -= offset;
                        self.index -= len.get() as usize;
                        self.past_len -= 1;
                        self.zeroes = 0;
                        match iter.next_back() {
                            Some(IterItem { offset, .. }) => self.last_update - offset,
                            None => u64::MIN,
                        }
                    }
                    None => panic!(
                        "self.out_of_or_past_end_log should be the last, unreachable log entry"
                    ),
                }
            }
            // missed an update, should have been reported by PastLenLogs, do nothing here as the
            // user seems to have decided against panicking in that case
            Ordering::Greater => return false,
        };

        meta.past_len_limits().push_past_len_update(
            self.update_state.expect("todo"),
            PastLenLimit::log_limits(backward_limit, meta.now()),
        );

        true
    }

    /// Checks at [`RevDirection::FORWARD_LOG`] if this log has been updated at this frame.
    ///
    /// Returns `Ok(true)` if that is the case or `Ok(false)` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `Ok(false)` then as well.
    ///
    /// If this returns an error, an update has been missed. See [`MissedUpdate`].
    ///
    /// If this log is potenitally updated more than once per frame, use this method in the fitting
    /// amount of `if` cases or with a `while` loop.
    ///
    /// See the [type docs] for an example.
    ///
    /// [`RevDirection::FORWARD_LOG`]: crate::meta::RevDirection::FORWARD_LOG
    /// [type docs]: PastLenLog
    #[track_caller]
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        // set next forward_limit if this method returns true
        let mut iter = OffsetIter(self.offset_bytes.range(self.index..));
        let forward_limit = match iter.next() {
            Some(IterItem { offset: 0, len }) => match self.last_update.cmp(&meta.now()) {
                // did not yet reach the next future frame in the log
                Ordering::Greater => return false,
                Ordering::Equal => {
                    self.past_len += 1;
                    if self.zeroes < len.get() as u8 - 1 {
                        self.zeroes += 1;
                        meta.now()
                    } else {
                        self.index += 1;
                        self.zeroes = 0;
                        match iter.next() {
                            Some(IterItem { offset, .. }) => meta.now() + offset,
                            None => u64::MAX,
                        }
                    }
                }
                // missed an update, should have been reported by PastLenLogs, do nothing here as
                // the user seems to have decided against panicking in that case
                Ordering::Less => return false,
            },
            Some(IterItem { offset, len }) => {
                let frame = self.last_update + offset;
                match frame.cmp(&meta.now()) {
                    // did not yet reach the next future frame in the log
                    Ordering::Greater => return false,
                    Ordering::Equal => {
                        self.last_update = frame;
                        self.index += len.get() as usize;
                        self.past_len += 1;
                        self.zeroes = 0;
                        match iter.next() {
                            Some(IterItem { offset, .. }) => frame + offset,
                            None => u64::MAX,
                        }
                    }
                    // missed an update, should have been reported by PastLenLogs, do nothing here
                    // as the user seems to have decided against panicking in that case
                    Ordering::Less => return false,
                }
            }
            None if self.zeroes < self.zeroes_max => match self.last_update.cmp(&meta.now()) {
                // did not yet reach the next future frame in the log
                Ordering::Greater => return false,
                Ordering::Equal => {
                    self.past_len += 1;
                    self.zeroes += 1;
                    meta.now()
                }
                // missed an update, should have been reported by PastLenLogs, do nothing here as
                // the user seems to have decided against panicking in that case
                Ordering::Less => return false,
            },
            // reached end of log
            None => return false,
        };

        meta.past_len_limits().push_past_len_update(
            self.update_state.expect("todo"),
            PastLenLimit::log_limits(meta.now(), forward_limit),
        );
        true
    }

    pub fn pre_update(&mut self, meta: &RevMeta) {
        match meta.update_past_len_state(&mut self.update_state, self.last_update) {
            PreUpdateVariant::RemoveLog => {
                self.offset_bytes.clear();
                self.out_of_or_past_end_log = 0;
                self.last_update = 0;
                self.index = 0;
                self.past_len = 0;
                self.zeroes = 0;
                self.zeroes_max = 0;
                self.update_state = None;
            }
            PreUpdateVariant::RemoveFuture => {
                if self.offset_bytes.len() > self.index {
                    self.offset_bytes.truncate(self.index);
                    self.zeroes = 0;
                }
                self.zeroes_max = self.zeroes;
            }
            PreUpdateVariant::Nothing => {}
        }
    }
}

#[cfg(test)]
mod test {
    use bevy::ecs::change_detection::MaybeLocation;

    use crate::meta::{RevDirection, RevQueue};

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

        fn item(offset: u64, len: u64) -> IterItem {
            IterItem {
                offset,
                len: NonZeroU64::new(len).unwrap(),
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
            item(0, MAX_ZEROES_PER_BYTE as u64),
        ];

        assert!(OffsetIter(deque.iter()).eq(expected.iter().cloned()));

        assert!(
            OffsetIter(deque.iter())
                .rev()
                .eq(expected.into_iter().rev())
        );
    }

    struct MetaAndLog {
        meta: RevMeta,
        past_len_log: PastLenLog,
        last_update: MaybeLocation,
    }

    // needed to get the same location in MetaAndLog and PastLenLogLimits
    impl PastLenLog {
        #[track_caller]
        fn forward_set_location(
            &mut self,
            meta: &RevMeta,
            past_len: u64,
            last_update: &mut MaybeLocation,
        ) {
            assert_eq!(self.update_and_get_past_len(meta), past_len);
            *last_update = MaybeLocation::caller();
        }
        #[track_caller]
        fn forward_log_set_location(&mut self, meta: &RevMeta, last_update: &mut MaybeLocation) {
            assert_eq!(self.forward_log(meta), true);
            *last_update = MaybeLocation::caller();
        }
        #[track_caller]
        fn backward_log_set_location(&mut self, meta: &RevMeta, last_update: &mut MaybeLocation) {
            assert_eq!(self.backward_log(meta), true);
            *last_update = MaybeLocation::caller();
        }
    }

    impl MetaAndLog {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(NonZeroU64::new(max_world_states), false),
                past_len_log: PastLenLog::new(),
                last_update: MaybeLocation::caller(),
            }
        }
        fn forward<const N: usize>(&mut self, past_lens: [u64; N]) {
            self.meta.set_queue(RevQueue::RUN_NOT_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::NOT_LOG);
                self.past_len_log.pre_update(meta);
                for past_len in past_lens {
                    self.past_len_log
                        .forward_set_location(meta, past_len, &mut self.last_update);
                }
            });
        }
        fn forward_log(&mut self, updates: u64) {
            // test cases where not all updates ran
            if updates > 0 {
                let missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        for _ in 0..insufficient_updates {
                            assert_eq!(self.past_len_log.forward_log(meta), true);
                        }
                    });

                    // despite the error, RevMeta and PastLenLog were updated and this has to be undone to
                    // continue testing
                    self.last_update = missed.last_update;
                    self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        for _ in 0..insufficient_updates {
                            assert_eq!(self.past_len_log.backward_log(meta), true);
                        }
                        // assert no more updates would run
                        assert_eq!(self.past_len_log.forward_log(meta), false);
                    });
                }
            }

            // test case where all updates ran
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::FORWARD_LOG);
                for _ in 0..updates {
                    self.past_len_log
                        .forward_log_set_location(meta, &mut self.last_update);
                }
                // assert no more updates would run
                assert_eq!(self.past_len_log.forward_log(meta), false);
            });
        }
        fn backward_log(&mut self, updates: u64) {
            // test cases where not all updates ran
            if updates > 0 {
                let missed = self.new_missed();

                println!("BEFORE: {:#?}", self.meta);
                for insufficient_updates in 0..updates {
                    println!("{insufficient_updates}");
                    self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        for _ in 0..insufficient_updates {
                            assert_eq!(self.past_len_log.backward_log(meta), true);
                        }
                    });

                    // despite the error, RevMeta and PastLenLog were updated and this has to be undone to
                    // continue testing
                    self.last_update = missed.last_update;
                    self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        for _ in 0..insufficient_updates {
                            assert_eq!(self.past_len_log.forward_log(meta), true);
                        }
                        // assert no more updates would run
                        assert_eq!(self.past_len_log.backward_log(meta), false);
                    });
                    println!("AFTER: {:#?}", self.meta);
                }
            }

            // test case where all updates ran
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::BackwardLog);
                for _ in 0..updates {
                    self.past_len_log
                        .backward_log_set_location(meta, &mut self.last_update);
                }
                // assert no more updates would run
                assert_eq!(self.past_len_log.backward_log(meta), false);
            });
        }
        fn new_missed(&self) -> PastLenLogMissed {
            PastLenLogMissed {
                internal_id: self.past_len_log.update_state.unwrap().id,
                last_update: self.last_update,
            }
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut meta_and_log = MetaAndLog::new(4);

        meta_and_log.forward([1]); // frame #1
        meta_and_log.forward([2, 3]); // frame #2
        meta_and_log.forward([4, 5]);
        meta_and_log.forward([]);
        // shortened log of runs from frame #1 and #2 --> past_len -= 3
        meta_and_log.forward([3, 4, 5]);

        meta_and_log.backward_log(3);
        meta_and_log.backward_log(0);
        meta_and_log.backward_log(2);

        meta_and_log.forward_log(2);
        meta_and_log.forward_log(0);
        meta_and_log.forward_log(3);

        meta_and_log.backward_log(3);
        meta_and_log.backward_log(0);
        meta_and_log.backward_log(2);

        meta_and_log.forward([1]);
    }

    #[test]
    fn behaves_like_meta_if_updated_once_per_frame() {
        let mut meta_and_log = MetaAndLog::new(4);

        meta_and_log.forward([1]); // frame #1
        assert_eq!(meta_and_log.meta.past_len(), 1);

        meta_and_log.forward([2]); // frame #2
        assert_eq!(meta_and_log.meta.past_len(), 2);

        meta_and_log.forward([3]);
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3]);
        assert_eq!(meta_and_log.meta.past_len(), 3);
    }

    /*
    #[test]
    fn missed_update_in_log() {
        let mut meta_and_log = MetaAndLog::new(4);

        meta_and_log.forward(Ok([1])); // frame #1
        meta_and_log.forward(Ok([2])); // frame #2
        meta_and_log.forward(Ok([3])); // frame #3

        meta_and_log.forward(Err(()));

        meta_and_log.backward_log(Ok(1));
        meta_and_log.backward_log(Ok(1));
        meta_and_log.backward_log(Ok(1));

        // -- log edge --
        let err = MissedUpdate(1);
        meta_and_log.forward_log_miss_frame();
        meta_and_log.forward_log(Err(err));
        meta_and_log.forward(Err(()));
    }

    #[test]
    fn missed_update_out_of_log() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward(Ok([1]));

        meta_and_log.backward_log(Ok(1));
        meta_and_log.forward_log_miss_frame();

        meta_and_log.forward(Ok([]));

        // -- log edge --
        meta_and_log.forward(Ok([]));
        meta_and_log.forward(Ok([]));

        meta_and_log.forward(Err(PastLenNotLogError::MissedUpdateForwardLog(
            MissedUpdate(1),
        )));
    }

    #[test]
    fn no_missed_frame_false_positive_in_log() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward(Ok([1]));

        meta_and_log.backward_log(Ok(1));

        meta_and_log.forward(Ok([]));
        // should not detect a missed forward log update
        meta_and_log.forward(Ok([1]));
    }

    #[test]
    fn no_missed_frame_false_positive_out_of_log() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward(Ok([1]));

        meta_and_log.backward_log(Ok(1));

        meta_and_log.forward(Ok([]));

        // -- log edge --
        meta_and_log.forward(Ok([]));
        meta_and_log.forward(Ok([]));
        // should not detect a missed forward log update
        meta_and_log.forward(Ok([1]));
    }
    */
}
