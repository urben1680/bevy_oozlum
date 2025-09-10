use crate::{log::PreUpdateVariant, meta::RevMeta};
use core::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::{TryReserveError, VecDeque},
    fmt::Display,
};

pub(crate) mod limits;
mod offset;

use bevy::ecs::change_detection::MaybeLocation;
use limits::*;
use offset::*;

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

impl Display for PastLenLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.id() {
            None => write!(f, "PastLenLog #Uninit"),
            Some(id) => write!(f, "PastLenLog #{id}"),
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

    pub fn id(&self) -> Option<u32> {
        self.update_state.map(|state| state.id)
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
        self.update_and_get_past_len_with_caller(meta, MaybeLocation::caller())
    }

    fn update_and_get_past_len_with_caller(
        &mut self,
        meta: &RevMeta,
        caller: MaybeLocation,
    ) -> u64 {
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
                self.past_len -= len.get() as u64;
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
            PastLenLimit::new_not_log(meta.now(), caller),
        );
        let offset = meta.now() - self.last_update;
        self.last_update = meta.now();
        self.past_len += 1;

        push_offset(
            &mut self.offset_bytes,
            &mut self.index,
            &mut self.zeroes,
            &mut self.zeroes_max,
            offset,
        );

        self.past_len
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
        self.backward_log_with_caller(meta, MaybeLocation::caller())
    }

    fn backward_log_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> bool {
        let now_plus_1 = meta.now() + 1;
        // set next backward_limit if this method returns true
        let backward_limit = match self.last_update.cmp(&now_plus_1) {
            // did not yet reach the next past frame in the log, may be at end of reachable log
            Ordering::Less => return false,
            Ordering::Equal if self.zeroes > 0 => {
                self.zeroes -= 1;
                now_plus_1
            }
            Ordering::Equal => {
                let mut iter = OffsetIter(self.offset_bytes.range(..self.index));
                match iter.next_back() {
                    Some(IterItem { offset: 0, len }) => {
                        self.index -= 1;
                        self.zeroes = len.get() - 1;
                        now_plus_1
                    }
                    Some(IterItem { offset, len }) => {
                        self.last_update -= offset;
                        self.index -= len.get() as usize;
                        self.zeroes = 0;
                        self.last_update
                    }
                    None => panic!(
                        "self.out_of_or_past_end_log should be the last, unreachable log entry"
                    ),
                }
            }
            // missed an update, should have been reported by PastLenLogLimits, do nothing here as the
            // user seems to have decided against panicking in that case
            Ordering::Greater => return false,
        };
        self.past_len -= 1;
        meta.past_len_limits().push_past_len_update(
            self.update_state.expect("todo"),
            PastLenLimit::new_log(backward_limit, meta.now(), caller),
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
        self.forward_log_with_caller(meta, MaybeLocation::caller())
    }

    fn forward_log_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> bool {
        let now_minus_1 = meta.now() - 1;
        // set next forward_limit if this method returns true
        let mut iter = OffsetIter(self.offset_bytes.range(self.index..));
        let forward_limit = match iter.next() {
            Some(IterItem { offset: 0, len }) => match self.last_update.cmp(&meta.now()) {
                // did not yet reach the next future frame in the log
                Ordering::Greater => return false,
                Ordering::Equal => {
                    if self.zeroes < len.get() as u8 - 1 {
                        self.zeroes += 1;
                        now_minus_1
                    } else {
                        self.index += 1;
                        self.zeroes = 0;
                        match iter.next() {
                            Some(IterItem { offset, .. }) => now_minus_1 + offset,
                            None if self.zeroes_max == 0 => u64::MAX,
                            None => now_minus_1,
                        }
                    }
                }
                // missed an update, should have been reported by PastLenLogLimits, do nothing here as
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
                        self.zeroes = 0;
                        match iter.next() {
                            Some(IterItem { offset, .. }) => frame - 1 + offset,
                            None if self.zeroes_max == 0 => u64::MAX,
                            None => frame - 1,
                        }
                    }
                    // missed an update, should have been reported by PastLenLogLimits, do nothing here
                    // as the user seems to have decided against panicking in that case
                    Ordering::Less => return false,
                }
            }
            None if self.zeroes < self.zeroes_max => match self.last_update.cmp(&meta.now()) {
                // did not yet reach the next future frame in the log
                Ordering::Greater => return false,
                Ordering::Equal => {
                    self.zeroes += 1;
                    if self.zeroes == self.zeroes_max {
                        u64::MAX
                    } else {
                        now_minus_1
                    }
                }
                // missed an update, should have been reported by PastLenLogLimits, do nothing here as
                // the user seems to have decided against panicking in that case
                Ordering::Less => return false,
            },
            // reached end of log
            None => return false,
        };
        self.past_len += 1;
        meta.past_len_limits().push_past_len_update(
            self.update_state.expect("todo"),
            PastLenLimit::new_log(meta.now(), forward_limit, caller),
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

    struct MetaAndLog {
        meta: RevMeta,
        past_len_log: PastLenLog,
        last_update: MaybeLocation,
    }

    impl MetaAndLog {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(core::num::NonZeroU64::new(max_world_states), false),
                past_len_log: PastLenLog::new(),
                last_update: MaybeLocation::caller(),
            }
        }
        #[track_caller]
        fn forward<const N: usize>(&mut self, past_lens: [u64; N], clear: bool) {
            let caller = MaybeLocation::caller();
            let queue = if clear {
                RevQueue::CLEAR_THEN_RUN
            } else {
                RevQueue::RUN_NOT_LOG
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::NOT_LOG);
                self.past_len_log.pre_update(meta);
                for past_len in past_lens {
                    assert_eq!(
                        self.past_len_log
                            .update_and_get_past_len_with_caller(meta, caller),
                        past_len,
                        "{:#?}",
                        self.past_len_log
                    );
                    self.last_update = caller;
                }
            });
        }
        #[track_caller]
        fn forward_log(&mut self, updates: u64) {
            let caller = MaybeLocation::caller();

            // test cases where not all updates ran
            if updates > 0 {
                let mut missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    if insufficient_updates > 0 {
                        missed.last_update = caller;
                    }
                    self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        for _ in 0..insufficient_updates {
                            missed.last_update = caller;
                            assert_eq!(
                                self.past_len_log.forward_log_with_caller(meta, caller),
                                true
                            );
                        }
                    });
                    missed.last_update = self.last_update;

                    // despite the error, RevMeta and PastLenLog were updated and this has to be undone to
                    // continue testing
                    self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        for _ in 0..insufficient_updates {
                            assert_eq!(
                                self.past_len_log
                                    .backward_log_with_caller(meta, self.last_update),
                                true
                            );
                        }
                        // assert no more updates would run
                        assert_eq!(self.past_len_log.backward_log(meta), false);
                    });
                }
            }

            // test case where all updates ran
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::FORWARD_LOG);
                for _ in 0..updates {
                    self.past_len_log.forward_log_with_caller(meta, caller);
                    self.last_update = caller;
                }
                // assert no more updates would run
                assert_eq!(self.past_len_log.forward_log(meta), false);
            });
        }
        #[track_caller]
        fn backward_log(&mut self, updates: u64) {
            let caller = MaybeLocation::caller();

            // test cases where not all updates ran
            if updates > 0 {
                let mut missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    if insufficient_updates > 0 {
                        missed.last_update = caller;
                    }
                    self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        for _ in 0..insufficient_updates {
                            missed.last_update = caller;
                            assert_eq!(
                                self.past_len_log.backward_log_with_caller(meta, caller),
                                true
                            );
                        }
                    });
                    missed.last_update = self.last_update;

                    // despite the error, RevMeta and PastLenLog were updated and this has to be undone to
                    // continue testing
                    self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        for _ in 0..insufficient_updates {
                            assert_eq!(
                                self.past_len_log
                                    .forward_log_with_caller(meta, self.last_update),
                                true
                            );
                        }
                        // assert no more updates would run
                        assert_eq!(self.past_len_log.forward_log(meta), false);
                    });
                }
            }

            // test case where all updates ran
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::BackwardLog);
                for _ in 0..updates {
                    self.past_len_log.backward_log_with_caller(meta, caller);
                    self.last_update = caller;
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

        meta_and_log.forward([1], false); // frame #1
        meta_and_log.forward([2, 3], false); // frame #2
        meta_and_log.forward([4, 5], false);
        meta_and_log.forward([], false);
        // shortened log of runs from frame #1 and #2 --> past_len -= 3
        meta_and_log.forward([3, 4, 5], false);

        meta_and_log.backward_log(3);
        meta_and_log.backward_log(0);
        meta_and_log.backward_log(2);

        meta_and_log.forward_log(2);
        meta_and_log.forward_log(0);
        meta_and_log.forward_log(3);

        meta_and_log.backward_log(3);
        meta_and_log.backward_log(0);

        meta_and_log.forward([3], false);

        meta_and_log.backward_log(1);

        meta_and_log.forward([], false); // should unset future limit
        meta_and_log.forward([], false);

        meta_and_log.backward_log(0);
        meta_and_log.backward_log(0);

        meta_and_log.forward_log(0);
        meta_and_log.forward_log(0);

        meta_and_log.forward([1, 2], false);
        meta_and_log.forward([1, 2], true);

        meta_and_log.backward_log(2);

        meta_and_log.forward_log(2);
    }

    #[test]
    fn behaves_like_meta_if_updated_once_per_frame() {
        let mut meta_and_log = MetaAndLog::new(4);

        meta_and_log.forward([1], false);
        assert_eq!(meta_and_log.meta.past_len(), 1);

        meta_and_log.forward([2], false);
        assert_eq!(meta_and_log.meta.past_len(), 2);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);
    }
}
