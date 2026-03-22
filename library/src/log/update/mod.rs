use core::{
    fmt::{Debug, Display},
    num::NonZeroU64,
};
use std::{collections::TryReserveError, panic::Location};

use bevy_ecs::change_detection::MaybeLocation;

use crate::{log::update::offset::OffsetLog, meta::RevMeta};

pub use limit::UpdateLogId;
use limit::*;

pub(super) mod limit;
mod offset;

/// A log that keeps track when it was updated and provides an alternative value to
/// [`MetaPastLen`](crate::meta::MetaPastLen) for when these updates do not happen exactly once per
/// [`RevUpdate`](crate::schedule::RevUpdate).
///
/// This type is usually accompied by another log that would grow too large if
/// [`PastLen`](crate::meta::PastLen) from
/// [`RevDirection::Forward`](crate::meta::RevDirection::Forward) was used when it actually updates
/// much more rarely. Another use case can be when it runs arbitrarily often per frame and there is
/// no other way to determine to which length a log should be at most when updating.
///
/// If an update is missed, for example when the scope of the log is behind complicated and
/// error-prone scheduling and is just not reached when it should, [`RevMeta::update`] and from that
/// [`RevMeta::run_rev_update`] will detect this at the end of the schedule and return an error.
///
/// # Example
///
/// The following system reacts on messages. These may not be written for many frames, but then
/// again they could appear in a large amount in a single frame. One could use a
/// [`TransitionsLog`](super::TransitionsLog) for that and just extend it with an empty iterator if
/// there are no messages.
///
/// But if this system in turn is also used with a run condition, then it is impossible to pick a
/// good `past_len` value that makes sure not too many messages are drained or the log grows way
/// beyond what is needed.
///
/// For that case and other comparable ones, the solution is to pair the log(s) with a `UpdateLog`.
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
///         RevDirection::Forward { .. } => {
///             if !messages.is_empty() {
///                 let iter = messages.read().cloned().inspect(|my_message| {
///                     // use message
///                 });
///                 let past_len = update_log.forward_past_len(&meta);
///                 message_log.forward_extend(&meta, past_len, iter);
///             }
///         }
///         RevDirection::ForwardLog => {
///             if update_log.forward_log(&meta) {
///                 for my_message in message_log.forward_log(&meta)? {
///                     // use message
///                 }
///             }
///         }
///         RevDirection::BackwardLog => {
///             if update_log.backward_log(&meta) {
///                 for my_message in message_log.backward_log(&meta)? {
///                     // use message
///                 }
///             }
///         }
///     }
///     Ok(())
/// }
/// ```
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

    /// The state that is needed to clean up the log at [`Self::pre_update_to_clear`] and to push
    /// new limits to [`UpdateLogLimits`].
    update_state: Option<UpdateLogState>,
}

impl Display for UpdateLog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.id() {
            None => Display::fmt(UpdateLogId::UNINIT, f),
            Some(id) => Display::fmt(&id, f),
        }
    }
}

impl UpdateLog {
    /// Creates an empty log.
    pub const fn new() -> Self {
        Self {
            offsets: OffsetLog::new(),
            log_start: 0,
            last_update: 0,
            past_len: 0,
            update_state: None,
        }
    }

    /// Creates an empty log with space for at least `bytes_capacity` bytes.
    ///
    /// See [`VecDeque::with_capacity`](std::collections::VecDeque::with_capacity).
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
    /// See [`VecDeque::len`](std::collections::VecDeque::len).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_len(&self) -> usize {
        self.offsets.get_bytes().len()
    }

    /// Returns the number of bytes the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`](std::collections::VecDeque::capacity).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_capacity(&self) -> usize {
        self.offsets.get_bytes().capacity()
    }

    /// Returns `true` if the log contains no bytes.
    ///
    /// See [`VecDeque::is_empty`](std::collections::VecDeque::is_empty).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_is_empty(&self) -> bool {
        self.offsets.get_bytes().is_empty()
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve`](std::collections::VecDeque::reserve).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_reserve(&mut self, additional: usize) {
        self.offsets.get_bytes_mut().reserve(additional)
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve_exact`](std::collections::VecDeque::reserve_exact).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_reserve_exact(&mut self, additional: usize) {
        self.offsets.get_bytes_mut().reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve`](std::collections::VecDeque::try_reserve).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offsets.get_bytes_mut().try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve_exact`](std::collections::VecDeque::try_reserve_exact).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offsets.get_bytes_mut().try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`](std::collections::VecDeque::shrink_to).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_shrink_to(&mut self, min_capacity: usize) {
        self.offsets.get_bytes_mut().shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`](std::collections::VecDeque::shrink_to_fit).
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn bytes_shrink_to_fit(&mut self) {
        self.offsets.get_bytes_mut().shrink_to_fit()
    }

    /// The internal id of this log, which is only `Some` after the first update. The id will change
    /// when [`RevQueue::ClearThenRunForward`](crate::meta::RevQueue::ClearThenRunForward) /
    /// [`RevQueue::ClearThenPause`](crate::meta::RevQueue::ClearThenPause) is queued and applied.
    ///
    /// When this id changes, an info log with the id is written.
    ///
    /// The [`Display`] implemention of `UpdateLog` solely contains this id.
    ///
    /// This id is useful to identify missed updates from [`RevMeta::update`]. If
    /// [`RevMeta::run_rev_update`] is used, such errors are handled by the default error handler.
    pub fn id(&self) -> Option<UpdateLogId> {
        self.update_state.map(|update_state| update_state.id())
    }

    /// Update the log and return the updated length of the log as an alternative to
    /// [`RevMeta::past_len`].
    ///
    /// This is used during [`RevDirection::Forward`](crate::meta::RevDirection::Forward).
    #[track_caller]
    pub fn forward_past_len(&mut self, meta: &RevMeta) -> NonZeroU64 {
        let caller = MaybeLocation::caller().map(Some);
        self.forward_past_len_with_caller(meta, caller)
    }

    #[track_caller]
    pub(crate) fn forward_past_len_with_caller(
        &mut self,
        meta: &RevMeta,
        caller: MaybeLocation<Option<&'static Location>>,
    ) -> NonZeroU64 {
        let past_len;

        if self.not_log_should_clear(meta, caller) {
            // log is empty or all updates are out of log
            // it is not important what offset is pushed as long log_start + offset = now
            // asusming 1-offset streak are the most common offsets and that during
            // RevDirection::Forward the value of now is non-zero, start such a 1-offset streak here
            self.offsets.clear();
            self.offsets.push(1);
            self.log_start = meta.now() - 1; // now() is never 0 during RevDirection::Forward
            self.last_update = meta.now();
            self.past_len = 1;
            past_len = NonZeroU64::MIN;
        } else {
            let offset = meta.now() - self.last_update;
            self.offsets.push(offset);
            self.last_update = meta.now();
            past_len = NonZeroU64::new(self.past_len + 1).expect("should not overflow");
            self.past_len = past_len.get();
        }

        meta.update_log_limits().push_limit(
            self.update_state.as_mut().unwrap(), // set by not_log_should_clear
            UpdateLogLimit::new_forward(
                meta.now(),
                caller.map(|caller| caller.unwrap_or(DEFAULT_LOCATION)),
            ),
        );

        past_len
    }

    /// Sets/updates [`Self::update_state`] so it is `Some`.
    ///
    /// If this method returns `true`, call [`Self::clear`] or do comparable operations.
    #[track_caller]
    fn not_log_should_clear(
        &mut self,
        meta: &RevMeta,
        caller: MaybeLocation<Option<&'static Location>>,
    ) -> bool {
        self.pre_update_to_clear(meta, caller) || self.last_update < meta.past_end() || {
            if self.log_start <= meta.past_end() {
                let mut minus = meta.past_end() - self.log_start;
                self.past_len -= self.offsets.truncate_past(&mut minus);
                self.log_start += minus;
            }
            self.past_len == 0
        }
    }

    /// Checks at [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) if this log
    /// has been updated at this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `false` then as well.
    #[track_caller]
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        let caller = MaybeLocation::caller().map(Some);
        self.backward_log_with_caller(meta, caller)
    }

    #[track_caller]
    pub(crate) fn backward_log_with_caller(
        &mut self,
        meta: &RevMeta,
        caller: MaybeLocation<Option<&'static Location>>,
    ) -> bool {
        if self.pre_update_to_clear(meta, caller) {
            self.clear();
            return false;
        }

        if self.past_len == 0 {
            // at the past end of the log
            return false;
        };

        let now_plus_1 = meta.now() + 1;

        if self.last_update != now_plus_1 {
            // if last_update is larger than now, an update was missed but it is up to the RevMeta
            // update to report on that
            return false;
        }

        let mut iter = self.offsets.now_to_past();
        self.last_update -= iter.next().unwrap();
        iter.sync();
        self.past_len -= 1;
        let backward_limit = iter.next().map_or(0, |_| self.last_update);

        meta.update_log_limits().push_limit(
            self.update_state.as_mut().unwrap(), // set by pre_update_to_clear
            UpdateLogLimit::new_log(
                backward_limit,
                meta.now(),
                caller.map(|caller| caller.unwrap_or(DEFAULT_LOCATION)),
            ),
        );

        true
    }

    /// Checks at [`RevDirection::ForwardLog`](crate::meta::RevDirection::ForwardLog) if this log
    /// has been updated at this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `false` then as well.
    #[track_caller]
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        let caller = MaybeLocation::caller().map(Some);
        self.forward_log_with_caller(meta, caller)
    }

    pub(crate) fn forward_log_with_caller(
        &mut self,
        meta: &RevMeta,
        caller: MaybeLocation<Option<&'static Location>>,
    ) -> bool {
        if self.pre_update_to_clear(meta, caller) {
            self.clear();
            return false;
        }

        let mut iter = self.offsets.now_to_future();
        match iter.next() {
            Some(offset) => {
                let frame = self.last_update + offset;
                if frame != meta.now() {
                    // if log_start is less than now, an update was missed but it is up to the
                    // RevMeta update to report on that
                    return false;
                }
                self.last_update = frame;
            }
            None => return false,
        }

        iter.sync();
        let forward_limit = iter.next().map_or(u64::MAX, |offset| {
            // order is important, offset may be 0 but now() is never 0 during ForwardLog
            meta.now() + offset - 1
        });
        self.past_len += 1;

        meta.update_log_limits().push_limit(
            self.update_state.as_mut().unwrap(), // set by pre_update_to_clear
            UpdateLogLimit::new_log(
                meta.now(),
                forward_limit,
                caller.map(|caller| caller.unwrap_or(DEFAULT_LOCATION)),
            ),
        );

        true
    }

    /// Shorten the log when log exits or clears were missed.
    ///
    /// If this method returns `true`, call [`Self::clear`] or do comparable operations.
    fn pre_update_to_clear(
        &mut self,
        meta: &RevMeta,
        caller: MaybeLocation<Option<&'static Location>>,
    ) -> bool {
        match meta.set_update_state(&mut self.update_state, caller) {
            PreUpdateKind::RemoveLog => true,
            PreUpdateKind::RemoveFuture => {
                self.offsets.truncate_future();
                if self.bytes_is_empty() {
                    self.log_start = 0;
                    self.last_update = 0;
                    self.past_len = 0;
                }
                false
            }
            PreUpdateKind::Nothing => false,
        }
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

// Reversible systems should never miss to run at the expected frame.
// If you followed an error to this line, you may have encountered a bug, please report it.
const DEFAULT_LOCATION: &'static Location = Location::caller();

/// Defines in which way a log has to be adjusted to reflect new changes to
/// [`RevMeta`](crate::meta::RevMeta) since the last time the log was updated.
pub(crate) enum PreUpdateKind {
    /// Keep the log unchanged
    Nothing,

    /// Remove log entries that are in the future
    RemoveFuture,

    /// Remove all log entries.
    RemoveLog,
}

#[cfg(test)]
mod test {
    use crate::meta::RevQueue;

    use super::*;

    #[derive(Debug)]
    struct MetaAndLog {
        meta: RevMeta,
        update_log: UpdateLog,
        last_update: MaybeLocation,
    }

    impl MetaAndLog {
        fn new(max_past_len: u64) -> Self {
            Self {
                meta: RevMeta::new(max_past_len, false),
                update_log: UpdateLog::new(),
                last_update: MaybeLocation::caller(),
            }
        }
        #[track_caller]
        fn forward<const N: usize>(&mut self, past_lens: [u64; N], clear: bool) {
            let caller = MaybeLocation::caller();
            let queue = if clear {
                RevQueue::ClearThenRunForward
            } else {
                RevQueue::RunForward
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, _| {
                for past_len in past_lens {
                    let past_len = NonZeroU64::new(past_len).unwrap();
                    let actual = self
                        .update_log
                        .forward_past_len_with_caller(meta, caller.map(Some));
                    assert_eq!(actual, past_len);
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
                    if insufficient_updates == 1 {
                        missed.last_update = caller;
                    }
                    self.meta.set_queue(RevQueue::RunForwardLog);
                    self.meta.update_ref(Err(missed), |meta, _| {
                        for _ in 0..insufficient_updates {
                            assert_eq!(
                                self.update_log
                                    .forward_log_with_caller(meta, caller.map(Some)),
                                true
                            );
                        }
                    });
                    self.revert(insufficient_updates, false);
                }
            }

            // test where all updates ran
            self.meta.set_queue(RevQueue::RunForwardLog);
            self.meta.update_ref(Ok(true), |meta, _| {
                for _ in 0..updates {
                    assert!(
                        self.update_log
                            .forward_log_with_caller(meta, caller.map(Some))
                    );
                }
                // assert no more updates would run
                assert_eq!(self.update_log.forward_log(meta), false);
            });

            if updates != 0 {
                self.last_update = caller;
            }
        }
        #[track_caller]
        fn backward_log(&mut self, updates: u64) {
            let caller = MaybeLocation::caller();

            // test cases where not all updates ran
            if updates > 0 {
                let mut missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    if insufficient_updates == 1 {
                        missed.last_update = caller;
                    }
                    self.meta.set_queue(RevQueue::RunBackwardLog);
                    self.meta.update_ref(Err(missed), |meta, _| {
                        for _ in 0..insufficient_updates {
                            assert_eq!(
                                self.update_log
                                    .backward_log_with_caller(meta, caller.map(Some)),
                                true
                            );
                        }
                    });
                    self.revert(insufficient_updates, true);
                }
            }

            // test where all updates ran
            self.meta.set_queue(RevQueue::RunBackwardLog);
            self.meta.update_ref(Ok(true), |meta, _| {
                for _ in 0..updates {
                    assert!(
                        self.update_log
                            .backward_log_with_caller(meta, caller.map(Some))
                    );
                }
                // assert no more updates would run
                assert_eq!(self.update_log.backward_log(meta), false);
            });

            if updates != 0 {
                self.last_update = caller;
            }
        }
        fn new_missed(&self) -> UpdateLogMissed {
            UpdateLogMissed {
                id: self.update_log.id().unwrap(),
                last_update: self.last_update,
            }
        }
        fn revert(&mut self, updates: u64, forward: bool) {
            let queue = if forward {
                RevQueue::RunForwardLog
            } else {
                RevQueue::RunBackwardLog
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, _| {
                if forward {
                    for _ in 0..updates {
                        assert_eq!(
                            self.update_log
                                .forward_log_with_caller(meta, self.last_update.map(Some)),
                            true,
                        );
                    }
                } else {
                    for _ in 0..updates {
                        assert_eq!(
                            self.update_log
                                .backward_log_with_caller(meta, self.last_update.map(Some)),
                            true,
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
