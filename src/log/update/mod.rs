use crate::{log::update::offset::OffsetLog, meta::RevMeta};
use alloc::collections::TryReserveError;
use bevy_ecs::change_detection::MaybeLocation;
use core::{
    cmp::Ordering,
    error::Error,
    fmt::{Debug, Display},
    num::NonZeroU64,
    panic::Location,
};

mod offset;

#[cfg(feature = "track_update_logs")]
pub(super) mod limit;

#[cfg(feature = "track_update_logs")]
use limit::*;

/// A log that keeps track when it was updated and provides the value for the `past_len` argument of
/// [`TransitionLog::forward_push`]/[`TransitionsLog::forward_extend`]. This is useful for when
/// these logs are *not* updated exactly once per [`RevUpdate`](crate::schedule::RevUpdate).
///
/// This log is also useful alone as a compact `TransitionLog<bool>`.
///
/// If an update is missed, for example when the scope of the log is behind complicated and
/// error-prone scheduling and is just not reached when it should, [`RevMeta::update`] and from that
/// [`run_rev_update`] will detect this at the end of the schedule and return an error.
///
/// # Example
///
/// The following system reacts on messages. These may not be written for many frames, but then
/// again they could appear in a large amount in a single frame. One could use a [`TransitionsLog`]
/// for that and just extend it with an empty iterator if there are no messages.
///
/// But if this system in turn is also used with a run condition, then it is impossible to pick a
/// good `past_len` value that makes sure not too many messages are drained or the log grows way
/// beyond what is needed.
///
/// For that case and other comparable ones, the solution is to pair the log(s) with an `UpdateLog`.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_ecs::error::Result;
/// # use bevy_oozlum::prelude::*;
/// # #[derive(Message, Clone)]
/// # struct MyMessage;
/// fn my_system(
///     meta: Res<RevMeta>,
///     mut messages: MessageReader<MyMessage>,
///     mut update_log: Local<UpdateLog>,
///     mut message_log: Local<TransitionsLog<MyMessage>>,
/// ) -> Result {
///     match meta.running_direction() {
///         RevDirection::NotLog(_) => {
///             if !messages.is_empty() {
///                 let iter = messages.read().cloned().inspect(|my_message| {
///                     // use message
///                 });
///                 let past_len = update_log.forward_past_len(&meta);
///                 message_log.forward_extend(&meta, past_len, iter);
///             }
///         }
///         RevDirection::ForwardLog => {
///             if update_log.forward_log(&meta)? {
///                 for my_message in message_log.forward_log(&meta)? {
///                     // use message
///                 }
///             }
///         }
///         RevDirection::BackwardLog => {
///             if update_log.backward_log(&meta)? {
///                 for my_message in message_log.backward_log(&meta)? {
///                     // use message
///                 }
///             }
///         }
///     }
///     Ok(())
/// }
/// ```
///
/// [`TransitionLog::forward_push`]: super::TransitionLog::forward_push
/// [`TransitionsLog::forward_extend`]: super::TransitionsLog::forward_extend
/// [`run_rev_update`]: crate::schedule::run_rev_update
/// [`TransitionsLog`]: super::TransitionsLog
#[derive(Default, Debug)]
pub struct UpdateLog {
    /// Offsets that need to be added or subtracted from [`Self::last_update`] to calculate at which
    /// frame the log is expected to be updated.
    ///
    /// For the encoding, see the [`offset`] module.
    offsets: OffsetLog,

    /// The most past frame that can be reached by iterating the offsets to the past from
    /// [`Self::last_update`]. This frame is equal or less than [`RevMeta::past_end`] and may never
    /// have been a frame this log was updated at, or was updated at but this frame is no longer
    /// reachable when traversing the global log.
    ///
    /// This frame is used to determine if and how many past offsets can be truncated.
    log_start: u64,

    /// The chronological last frame in the past this log got updated at.
    last_update: u64,

    /// The length of the log which is what this log is keeping track of.
    past_len: u64,

    /// Contains the most recent global count of log exits that was witnessed..
    ///
    /// See [`RevMeta::log_exits`](crate::meta::RevMeta::log_exits).
    witnessed_log_exits: u64,

    /// Contains the most recent global count of log clears that was witnessed.
    ///
    /// See [`RevMeta::log_clears`](crate::meta::RevMeta::log_clears).
    witnessed_log_clears: u64,

    /// The state that is needed to clean up the log at [`Self::pre_update_to_clear`] and to push
    /// new limits to [`UpdateLogLimits`].
    #[cfg(feature = "track_update_logs")]
    update_state: Option<UpdateLogState>,
}

impl UpdateLog {
    /// Creates an empty log.
    pub const fn new() -> Self {
        Self {
            offsets: OffsetLog::new(),
            log_start: 0,
            last_update: 0,
            past_len: 0,
            witnessed_log_exits: 0,
            witnessed_log_clears: 0,
            #[cfg(feature = "track_update_logs")]
            update_state: None,
        }
    }

    /// Creates an empty log with space for at least `bytes_capacity` bytes.
    ///
    /// See [`VecDeque::with_capacity`](alloc::collections::VecDeque::with_capacity).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn with_capacity(bytes_capacity: usize) -> Self {
        Self {
            offsets: OffsetLog::with_capacity(bytes_capacity),
            ..Self::new()
        }
    }

    /// Returns the number of bytes in the log.
    ///
    /// See [`VecDeque::len`](alloc::collections::VecDeque::len).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_len(&self) -> usize {
        self.offsets.get_bytes().len()
    }

    /// Returns the number of bytes the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`](alloc::collections::VecDeque::capacity).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_capacity(&self) -> usize {
        self.offsets.get_bytes().capacity()
    }

    /// Returns `true` if the log contains no bytes.
    ///
    /// See [`VecDeque::is_empty`](alloc::collections::VecDeque::is_empty).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_is_empty(&self) -> bool {
        self.offsets.get_bytes().is_empty()
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve`](alloc::collections::VecDeque::reserve).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_reserve(&mut self, additional: usize) {
        self.offsets.get_bytes_mut().reserve(additional)
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve_exact`](alloc::collections::VecDeque::reserve_exact).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_reserve_exact(&mut self, additional: usize) {
        self.offsets.get_bytes_mut().reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve`](alloc::collections::VecDeque::try_reserve).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offsets.get_bytes_mut().try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve_exact`](alloc::collections::VecDeque::try_reserve_exact).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offsets.get_bytes_mut().try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`](alloc::collections::VecDeque::shrink_to).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_shrink_to(&mut self, min_capacity: usize) {
        self.offsets.get_bytes_mut().shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`](alloc::collections::VecDeque::shrink_to_fit).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_shrink_to_fit(&mut self) {
        self.offsets.get_bytes_mut().shrink_to_fit()
    }

    /// Returns the most recent global count of log exits that was witnessed or `0`.
    ///
    /// See [`RevMeta::log_exits`].
    pub fn witnessed_log_exits(&self) -> u64 {
        self.witnessed_log_exits
    }

    /// Returns the most recent global count of log clears that was witnessed or `0`.
    ///
    /// See [`RevMeta::log_clears`].
    pub fn witnessed_log_clears(&self) -> u64 {
        self.witnessed_log_clears
    }

    /// Update the log and return the updated length of the log as an alternative to
    /// [`RevMeta::past_len`].
    ///
    /// This is used during [`RevDirection::NotLog`](crate::meta::RevDirection::NotLog).
    #[track_caller]
    pub fn forward_past_len(&mut self, meta: &RevMeta) -> NonZeroU64 {
        let caller = MaybeLocation::caller().map(Some);
        self.forward_past_len_with_caller(meta, caller)
    }

    #[track_caller]
    pub(crate) fn forward_past_len_with_caller(
        &mut self,
        meta: &RevMeta,
        caller: UpdateLocation,
    ) -> NonZeroU64 {
        if self.not_log_should_clear(meta, caller) {
            // log is empty or all updates are out of log
            // it is not important what offset is pushed as long log_start + offset = now
            // asusming 1-offset streak are the most common offsets and that during
            // RevDirection::NotLog the value of now is non-zero, start such a 1-offset streak here
            self.offsets.clear();
            self.offsets.push(1);
            self.log_start = meta.now() - 1; // now() is never 0 during RevDirection::NotLog
            self.last_update = meta.now();
            self.past_len = 1;
        } else {
            let offset = meta.now() - self.last_update;
            self.offsets.push(offset);
            self.last_update = meta.now();
            self.past_len += 1;
        }

        #[cfg(feature = "track_update_logs")]
        meta.update_log_limits().push_limit(
            self.update_state.as_mut().unwrap(), // set by not_log_should_clear
            UpdateLogLimit::new_forward(meta.now(), caller),
        );

        // past_len is either set to 1 or increased above, overflow is unexpected
        NonZeroU64::new(self.past_len).unwrap()
    }

    /// Sets/updates [`Self::update_state`] so it is `Some`.
    ///
    /// If this method returns `true`, call [`Self::clear`] or do comparable operations.
    #[track_caller]
    fn not_log_should_clear(&mut self, meta: &RevMeta, caller: UpdateLocation) -> bool {
        self.pre_update_to_clear(meta, caller) || self.last_update < meta.past_end() || {
            if self.log_start <= meta.past_end() {
                let mut minus = meta.past_end() - self.log_start;
                self.past_len -= self.offsets.truncate_past(&mut minus);
                self.log_start += minus;
            }
            self.past_len == 0
        }
    }

    /// Updates the log at [`RevDirection::BackwardLog`] if [`forward_past_len`] has been called at
    /// this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This will return `true` for as many
    /// times during the same frame as `forward_past_len` was called.
    ///
    /// This method is insensitive on on being called during frames outside its internal log and
    /// returns `false` in that case.
    ///
    /// [`RevDirection::BackwardLog`]: crate::meta::RevDirection::BackwardLog
    /// [`forward_past_len`]: Self::forward_past_len
    #[track_caller]
    pub fn backward_log(&mut self, meta: &RevMeta) -> Result<bool, UpdateMissedAt> {
        let caller = MaybeLocation::caller().map(Some);
        self.backward_log_with_caller(meta, caller)
    }

    /// See [`Self::backward_log`]. Set `caller` to `None` for crate-internal logs.
    pub(crate) fn backward_log_with_caller(
        &mut self,
        meta: &RevMeta,
        caller: UpdateLocation,
    ) -> Result<bool, UpdateMissedAt> {
        if self.pre_update_to_clear(meta, caller) {
            self.clear();
            return Ok(false);
        }

        if self.past_len == 0 {
            // at the past end of the log
            return Ok(false);
        };

        match self.last_update.cmp(&meta.now()) {
            Ordering::Less => return Ok(false),
            Ordering::Equal => {}
            Ordering::Greater => {
                return Err(UpdateMissedAt {
                    now: meta.now(),
                    missed: self.last_update,
                });
            }
        }

        let mut iter = self.offsets.now_to_past();
        self.last_update -= iter.next().unwrap(); // must be Some because self.past_len != 0
        iter.sync();
        self.past_len -= 1;

        #[cfg(feature = "track_update_logs")]
        meta.update_log_limits().push_limit(
            self.update_state.as_mut().unwrap(), // set by pre_update_to_clear
            UpdateLogLimit::new_log(
                iter.next().map_or(0, |_| self.last_update),
                meta.now() - 1,
                caller,
            ),
        );

        Ok(true)
    }

    /// Updates the log at [`RevDirection::ForwardLog`] if [`forward_past_len`] has been called at
    /// this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This will return `true` for as many
    /// times during the same frame as `forward_past_len` was called.
    ///
    /// This method is insensitive on on being called during frames outside its internal log and
    /// returns `false` in that case.
    ///
    /// [`RevDirection::ForwardLog`]: crate::meta::RevDirection::ForwardLog
    /// [`forward_past_len`]: Self::forward_past_len
    #[track_caller]
    pub fn forward_log(&mut self, meta: &RevMeta) -> Result<bool, UpdateMissedAt> {
        let caller = MaybeLocation::caller().map(Some);
        self.forward_log_with_caller(meta, caller)
    }

    /// See [`Self::forward_log`]. Set `caller` to `None` for crate-internal logs.
    pub(crate) fn forward_log_with_caller(
        &mut self,
        meta: &RevMeta,
        caller: UpdateLocation,
    ) -> Result<bool, UpdateMissedAt> {
        if self.pre_update_to_clear(meta, caller) {
            self.clear();
            return Ok(false);
        }

        let mut iter = self.offsets.now_to_future();
        match iter.next() {
            Some(offset) => {
                let frame = self.last_update + offset;
                match frame.cmp(&meta.now()) {
                    Ordering::Greater => return Ok(false),
                    Ordering::Equal => self.last_update = frame,
                    Ordering::Less => {
                        return Err(UpdateMissedAt {
                            now: meta.now(),
                            missed: frame,
                        });
                    }
                }
            }
            None => return Ok(false),
        }

        iter.sync();
        self.past_len += 1;

        #[cfg(feature = "track_update_logs")]
        meta.update_log_limits().push_limit(
            self.update_state.as_mut().unwrap(), // set by pre_update_to_clear
            UpdateLogLimit::new_log(
                meta.now(),
                iter.next().map_or(u64::MAX, |offset| {
                    // order is important, offset may be 0 but now() is never 0 during ForwardLog
                    meta.now() + offset - 1
                }),
                caller,
            ),
        );

        Ok(true)
    }

    /// Shorten the log when log exits or clears were missed.
    ///
    /// If this method returns `true`, call [`Self::clear`] or do comparable operations.
    fn pre_update_to_clear(&mut self, meta: &RevMeta, _caller: UpdateLocation) -> bool {
        let mut clear = false;

        if self.witnessed_log_clears < meta.log_clears() {
            // meta was cleared in the meantime
            self.witnessed_log_clears = meta.log_clears();
            self.witnessed_log_exits = meta.log_exits();
            clear = true;

            #[cfg(feature = "track_update_logs")]
            {
                self.update_state = None;
            }
        } else if self.witnessed_log_exits < meta.log_exits() {
            // meta ran at not-log in the meantime
            self.witnessed_log_exits = meta.log_exits();
            self.offsets.truncate_future();
            if self.bytes_is_empty() {
                self.log_start = 0;
                self.last_update = 0;
                self.past_len = 0;
            }
        }

        #[cfg(feature = "track_update_logs")]
        meta.update_log_limits()
            .set_update_state(&mut self.update_state, _caller);

        clear
    }

    /// Clear the log to be in the state of construction except of [`Self::update_state`] which is
    /// not reset.
    fn clear(&mut self) {
        self.offsets.clear();
        self.log_start = 0;
        self.last_update = 0;
        self.past_len = 0;
    }
}

/// An error type that may be returned by [`UpdateLog::forward_log`]/[`UpdateLog::backward_log`].
///
/// This can occur if a call to those methods was missed at a certain frame since the log was last
/// updated. Note however that this error is not 100% reliable, in some situations the above methods
/// return `Ok` even though an update was missed. Still, using the more reliable `track_update_logs`
/// feature is usually not needed in most cases unless [`UpdateLog`]s are used anywhere else than
/// `Local` parameters of reversible systems.
///
/// If `track_update_logs` is used, [`RevMeta::update`] (and thus the
/// [`run_rev_update`](crate::schedule::run_rev_update) system) returns an error at the frame an
/// update of any [`UpdateLog`] was missed. Because of this, issues are noticed before
/// [`UpdateMissedAt`] errors could be returned at updates.
///
/// Regardless of which error detection approach is picked, missed updates just as
/// [`OutOfLog`](super::OutOfLog) occurrences indicate an invalid world state.
///
/// See the [module level documentation](crate::log) for more information.
#[derive(Debug, Clone, Copy)]
pub struct UpdateMissedAt {
    /// The [current frame](RevMeta::now).
    pub now: u64,

    /// The frame this [`UpdateLog`] was expected to be updated when it was not.
    pub missed: u64,
}

impl Display for UpdateMissedAt {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "UpdateLog expected to be updated at {} but with {} this was missed",
            self.missed, self.now
        )
    }
}

impl Error for UpdateMissedAt {}

type UpdateLocation = MaybeLocation<Option<&'static Location<'static>>>;

#[cfg(all(test, feature = "track_update_logs"))]
mod test {
    use crate::meta::RevQueue;

    use super::*;

    #[derive(Debug)]
    struct MetaAndLog {
        meta: RevMeta,
        update_log: UpdateLog,
        last_update: UpdateLocation,
    }

    impl MetaAndLog {
        fn new(max_past_len: u64) -> Self {
            Self {
                meta: RevMeta::new(max_past_len),
                update_log: UpdateLog::new(),
                last_update: MaybeLocation::caller().map(Some),
            }
        }
        #[track_caller]
        fn forward<const N: usize>(&mut self, past_lens: [u64; N], clear: bool) {
            let caller = MaybeLocation::caller().map(Some);
            let queue = if clear {
                Some(RevQueue::ClearThenRunNotLog)
            } else {
                Some(RevQueue::RunNotLog)
            };
            self.meta.update_ref_or_missed(queue, Ok(true), |meta, _| {
                for past_len in past_lens {
                    let past_len = NonZeroU64::new(past_len).unwrap();
                    let actual = self.update_log.forward_past_len_with_caller(meta, caller);
                    assert_eq!(actual, past_len);
                    self.last_update = caller;
                }
            });
        }
        #[track_caller]
        fn forward_log(&mut self, updates: u64) {
            let caller = MaybeLocation::caller().map(Some);

            // test cases where not all updates ran
            if updates > 0 {
                let mut missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    if insufficient_updates == 1 {
                        missed.last_update = caller;
                    }
                    self.meta.update_ref_or_missed(
                        Some(RevQueue::RunForwardLog),
                        Err(missed),
                        |meta, _| {
                            for _ in 0..insufficient_updates {
                                assert!(
                                    self.update_log
                                        .forward_log_with_caller(meta, caller)
                                        .unwrap()
                                );
                            }
                        },
                    );
                    self.revert(insufficient_updates, false);
                }
            }

            // test where all updates ran
            self.meta
                .update_ref_or_missed(Some(RevQueue::RunForwardLog), Ok(true), |meta, _| {
                    for _ in 0..updates {
                        assert!(
                            self.update_log
                                .forward_log_with_caller(meta, caller)
                                .unwrap()
                        );
                    }
                    // assert no more updates would run
                    assert!(!self.update_log.forward_log(meta).unwrap());
                });

            if updates != 0 {
                self.last_update = caller;
            }
        }
        #[track_caller]
        fn backward_log(&mut self, updates: u64) {
            let caller = MaybeLocation::caller().map(Some);

            // test cases where not all updates ran
            if updates > 0 {
                let mut missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    if insufficient_updates == 1 {
                        missed.last_update = caller;
                    }
                    self.meta.update_ref_or_missed(
                        Some(RevQueue::RunBackwardLog),
                        Err(missed),
                        |meta, _| {
                            for _ in 0..insufficient_updates {
                                assert!(
                                    self.update_log
                                        .backward_log_with_caller(meta, caller)
                                        .unwrap()
                                );
                            }
                        },
                    );
                    self.revert(insufficient_updates, true);
                }
            }

            // test where all updates ran
            self.meta
                .update_ref_or_missed(Some(RevQueue::RunBackwardLog), Ok(true), |meta, _| {
                    for _ in 0..updates {
                        assert!(
                            self.update_log
                                .backward_log_with_caller(meta, caller)
                                .unwrap()
                        );
                    }
                    // assert no more updates would run
                    assert!(!self.update_log.backward_log(meta).unwrap());
                });

            if updates != 0 {
                self.last_update = caller;
            }
        }
        fn new_missed(&self) -> UpdateLogMissed {
            UpdateLogMissed {
                index: self.update_log.update_state.unwrap().index.get() as usize,
                last_update: self.last_update,
            }
        }
        fn revert(&mut self, updates: u64, forward: bool) {
            let queue = if forward {
                Some(RevQueue::RunForwardLog)
            } else {
                Some(RevQueue::RunBackwardLog)
            };
            self.meta.update_ref_or_missed(queue, Ok(true), |meta, _| {
                if forward {
                    for _ in 0..updates {
                        assert!(
                            self.update_log
                                .forward_log_with_caller(meta, self.last_update)
                                .unwrap(),
                        );
                    }
                } else {
                    for _ in 0..updates {
                        assert!(
                            self.update_log
                                .backward_log_with_caller(meta, self.last_update)
                                .unwrap(),
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward([1], false); // frame #1
        meta_and_log.forward([2, 3], false); // frame #2
        meta_and_log.forward([4, 5], false); // frame #3
        meta_and_log.forward([], false); // frame #4
        // shortened log of runs from frame #1 and #2 --> past_len -= 3
        meta_and_log.forward([3, 4, 5], false); // frame #5

        meta_and_log.backward_log(3); // undo frame #5
        meta_and_log.backward_log(0); // undo frame #4
        meta_and_log.backward_log(2); // undo frame #3

        meta_and_log.forward_log(2); // redo frame #3
        meta_and_log.forward_log(0); // redo frame #4
        meta_and_log.forward_log(3); // redo frame #5

        meta_and_log.backward_log(3); // undo frame #5
        meta_and_log.backward_log(0); // undo frame #4

        meta_and_log.forward([3], false); // frame #4

        meta_and_log.backward_log(1); // undo frame #4

        meta_and_log.forward([], false); // frame #4, should unset future limit
        meta_and_log.forward([], false); // frame #5

        meta_and_log.backward_log(0); // undo frame #5
        meta_and_log.backward_log(0); // undo frame #4

        meta_and_log.forward_log(0); // redo frame #4
        meta_and_log.forward_log(0); // redo frame #5

        meta_and_log.forward([1, 2], false); // frame #6
        meta_and_log.forward([1, 2], true); // frame #7

        meta_and_log.backward_log(2); // undo frame #7

        meta_and_log.forward_log(2); // redo frame #7
    }

    #[test]
    fn behaves_like_meta() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward([1], false);
        assert_eq!(meta_and_log.meta.past_len(), 1);

        meta_and_log.forward([2], false);
        assert_eq!(meta_and_log.meta.past_len(), 2);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);
    }

    #[test]
    fn behaves_like_meta_minus_gaps() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward([1], false);
        assert_eq!(meta_and_log.meta.past_len(), 1);

        meta_and_log.forward([], false);

        meta_and_log.forward([2], false); // missed one
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([2], false); // popped one
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3], false); // catched up
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([], false);

        meta_and_log.forward([], false);

        meta_and_log.forward([1], false); // popped one, missed two
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([2], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3], false); // catched up
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);
    }

    #[test]
    fn forward_truncates_future() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward([1], false);

        meta_and_log.backward_log(1);

        meta_and_log.forward([], false);

        meta_and_log.backward_log(0);

        meta_and_log.forward_log(0);
    }

    #[test]
    fn simple_undo_redo() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward([1], false);

        meta_and_log.backward_log(1);

        meta_and_log.forward_log(1);
    }

    #[test]
    fn full_truncate_future_clears() {
        let mut meta_and_log = MetaAndLog::new(3);

        meta_and_log.forward([], false);
        meta_and_log.forward([1], false);

        meta_and_log.backward_log(1);
        meta_and_log.backward_log(0);

        meta_and_log.forward([1], false);
    }
}
