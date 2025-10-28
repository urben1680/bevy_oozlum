use crate::meta::RevMeta;
use bevy_ecs::change_detection::MaybeLocation;
use core::fmt::{Debug, Display};
use std::collections::{TryReserveError, VecDeque};

pub use limits::UpdateLogId;
use limits::*;
use offset::*;

pub(super) mod limits;
mod offset;

/// A log that keeps track when it was updated and provides an alternative value to
/// [`RevMeta::past_len`] for when these updates do not happen exactly once per
/// [`RevUpdate`](crate::schedule::RevUpdate).
///
/// This type is usually accompied by another log that would grow too large if `RevMeta::past_len`
/// was used when it actually updates much more rarely. Another use case can be when it runs
/// arbitrarily often per frame and there is no other way to determine to which length a log should
/// be at most when updating.
///
/// If an update is missed, for example when the scope of the log is behind complicated and
/// error-prone scheduling and is just not reached when it should, [`RevMeta::update`] and from that
/// [`RevMeta::run_rev_update`] will detect this and return an error.
///
/// # Example
///
/// The following system reacts on messages. These may not be written for many frames, but then
/// again they could appear in a large amount in a single frame. One could use a
/// [`TransitionsLog`](super::TransitionsLog) for that and just extend it with an empty iterator if
/// there are no messages.
///
/// But if this system in turn is also used with a run condition, then it is impossible to pick a
/// good `max_past_len` value that makes sure not too many messages are drained or the log grows way
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
///         RevDirection::NOT_LOG => {
///             if !messages.is_empty() {
///                 let iter = messages.read().cloned().inspect(|my_message| {
///                     // use message
///                 });
///                 let past_len = update_log.forward_past_len(&meta);
///                 message_log.forward_extend(&meta, past_len, iter);
///             }
///         }
///         RevDirection::FORWARD_LOG => {
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
#[derive(Default)]
pub struct UpdateLog {
    /// Offsets that need to be added or subtracted from [`Self::last_update`] to calculate at which
    /// frame the log is expected to be updated.
    ///
    /// For the encoding, see the [`offset`] module.
    offset_bytes: VecDeque<u8>,

    /// The most past frame the log was updated. Each update truncates offsets until this value is
    /// larger than [`RevMeta::past_end`].
    log_start: u64,

    /// The chronological last frame in the past this log got updated at.
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

    /// The state that is needed to clean up the log at [`Self::pre_update`] and to push
    /// new limits to [`UpdateLogLimits`].
    update_state: Option<UpdateLogState>,
}

impl Debug for UpdateLog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UpdateLog")
            .field("offset_bytes", &self.offset_bytes)
            .field("offsets (decoded)", &OffsetIter(self.offset_bytes.iter()))
            .field("log_start", &self.log_start)
            .field("last_update", &self.last_update)
            .field("index", &self.index)
            .field("past_len", &self.past_len)
            .field("zeroes", &self.zeroes)
            .field("zeroes_max", &self.zeroes_max)
            .field("update_state", &self.update_state)
            .finish()
    }
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
            offset_bytes: VecDeque::new(),
            log_start: 0,
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
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn with_capacity(bytes_capacity: usize) -> Self {
        Self {
            offset_bytes: VecDeque::with_capacity(bytes_capacity),
            ..Self::new()
        }
    }

    /// Returns the number of bytes in the log.
    ///
    /// See [`VecDeque::len`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn len(&self) -> usize {
        self.offset_bytes.len()
    }

    /// Returns the number of bytes the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn capacity(&self) -> usize {
        self.offset_bytes.capacity()
    }

    /// Returns `true` if the log contains no bytes.
    ///
    /// See [`VecDeque::is_empty`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn is_empty(&self) -> bool {
        self.offset_bytes.is_empty()
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn reserve(&mut self, additional: usize) {
        self.offset_bytes.reserve(additional)
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve_exact`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn reserve_exact(&mut self, additional: usize) {
        self.offset_bytes.reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve_exact`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.offset_bytes.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn shrink_to_fit(&mut self) {
        self.offset_bytes.shrink_to_fit()
    }

    /// The internal id of this log, which is only `Some` after it first ran. The id will change
    /// when [`RevQueue::Clear`](crate::meta::RevQueue::Clear) is queued and applied.
    ///
    /// When this id changes, an info log with the id is written.
    ///
    /// This id is useful to identify missed updates from [`RevMeta::update`]. If
    /// [`RevMeta::run_rev_update`] is used, such errors are handled by the default error handler.
    pub fn id(&self) -> Option<UpdateLogId> {
        self.update_state.map(|update_state| update_state.id())
    }

    /// Update the log and return the updated length of the log as an alternative to
    /// [`RevMeta::past_len`].
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG) when the
    /// current scope has been determined for some operation to happen, most often in combination
    /// with another log that is updated next with the returned value.
    #[track_caller]
    pub fn forward_past_len(&mut self, meta: &RevMeta) -> u64 {
        self.forward_past_len_with_caller(meta, MaybeLocation::caller())
    }

    fn forward_past_len_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> u64 {
        if self.pre_update_to_clear(meta) || self.last_update <= meta.past_end() {
            // all updates are out of log
            self.offset_bytes.clear();
            self.log_start = meta.now();
            self.last_update = meta.now();
            self.index = 0;
            self.past_len = 1;
            self.zeroes = 0;
            self.zeroes_max = 0;
        } else if self.log_start > meta.past_end() {
            // no updates are out of log
            self.push_offset(meta.now() - self.last_update);
            self.last_update = meta.now();
            self.past_len += 1;
        } else {
            // some updates are out of log
            let iter = OffsetIter(self.offset_bytes.range(..self.index));
            let mut to_drain = 0;
            for IterItem { offset, len } in iter {
                if offset == 0 {
                    // Offsets of 0 are encoded differently, len is not the amount of bytes,
                    // which is actually always 1 here, but the amount of zero offsets in this
                    // one byte.
                    // We want to get rid of these offsets as well as they dont bring
                    // `self.log_start` any closer the limit.
                    to_drain += 1;
                    self.past_len -= len.get() as u64;
                    continue;
                }
                to_drain += len.get() as usize;
                self.past_len -= 1;
                self.log_start += offset;

                if self.log_start > meta.past_end() {
                    break;
                }
            }
            self.index -= to_drain;
            // todo: use truncate_front https://github.com/rust-lang/rust/issues/140667
            self.offset_bytes.drain(..to_drain);

            self.push_offset(meta.now() - self.last_update);
            self.last_update = meta.now();
            self.past_len += 1;
        }

        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_not_log(meta.now(), caller),
        );

        self.past_len
    }

    /// Checks at [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) if this log
    /// has been updated at this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `false` then as well.
    #[track_caller]
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        self.backward_log_with_caller(meta, MaybeLocation::caller())
    }

    fn backward_log_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> bool {
        if self.pre_update_to_clear(meta) {
            self.clear();
            return false;
        }

        if self.past_len == 0 {
            // at the past end of the log
            return false;
        }

        let now_plus_1 = meta.now() + 1;

        if self.last_update != now_plus_1 {
            // if last_update is larger than now, an update was missed but it is up to the RevMeta
            // update to report on that
            return false;
        }

        let backward_limit = if self.zeroes > 0 {
            // not all expected updates for this frame happened yet
            self.zeroes -= 1;
            now_plus_1
        } else {
            OffsetIter(self.offset_bytes.range(..self.index))
                .next_back()
                .map_or(0, |item| {
                    if item.offset == 0 {
                        self.index -= 1;
                        self.zeroes = item.len.get() - 1;
                        now_plus_1
                    } else {
                        self.last_update -= item.offset;
                        self.index -= item.len.get() as usize;
                        self.zeroes = 0;
                        self.last_update
                    }
                })
        };

        self.past_len -= 1;
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_log(backward_limit, meta.now(), caller),
        );

        true
    }

    /// Checks at [`RevDirection::FORWARD_LOG`](crate::meta::RevDirection::FORWARD_LOG) if this log
    /// has been updated at this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `false` then as well.
    #[track_caller]
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        self.forward_log_with_caller(meta, MaybeLocation::caller())
    }

    fn forward_log_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> bool {
        if self.pre_update_to_clear(meta) {
            self.clear();
            return false;
        }

        let now_minus_1 = meta.now() - 1;

        let forward_limit = if self.past_len == 0 {
            // at the past end of the log
            if self.log_start != meta.now() {
                // if log_start is less than now, an update was missed but it is up to the RevMeta
                // update to report on that
                return false;
            }
            self.next_forward_limit(OffsetIter(self.offset_bytes.iter()), now_minus_1)
        } else {
            let mut iter = OffsetIter(self.offset_bytes.range(self.index..));
            match iter.next() {
                // there is another update for the last_update frame to be equal with now
                Some(IterItem { offset: 0, len }) => {
                    if self.last_update != meta.now() {
                        // there was an updated missed at last_update but it is up to the RevMeta
                        // update to report on that
                        return false;
                    }
                    if self.zeroes < len.get() as u8 - 1 {
                        // not all updates this frame happened yet
                        self.zeroes += 1;
                        now_minus_1
                    } else {
                        // all updates this frame happened, unless there is another zero-offset next
                        self.index += 1;
                        self.zeroes = 0;
                        self.next_forward_limit(iter, now_minus_1)
                    }
                }
                // there is a next offset that may add to now
                Some(IterItem { offset, len }) => {
                    let frame = self.last_update + offset;
                    if frame != meta.now() {
                        // if log_start is less than now, an update was missed but it is up to the
                        // RevMeta update to report on that
                        return false;
                    }
                    self.last_update = frame;
                    self.index += len.get() as usize;
                    self.zeroes = 0;
                    self.next_forward_limit(iter, now_minus_1)
                }
                // there is another update for the last_update frame to be equal with now
                None if self.zeroes < self.zeroes_max => {
                    if self.last_update != meta.now() {
                        // there was an updated missed at self.last_update but it is up to the
                        // RevMeta update to report on that
                        return false;
                    }
                    self.zeroes += 1;
                    if self.zeroes == self.zeroes_max {
                        // there is no other update in the future
                        u64::MAX
                    } else {
                        // another update at this frame is expected
                        now_minus_1
                    }
                }
                // reached future end of log
                None => return false,
            }
        };

        self.past_len += 1;
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_log(meta.now(), forward_limit, caller),
        );

        true
    }

    fn next_forward_limit(&self, mut iter: OffsetIter, now_minus_1: u64) -> u64 {
        match iter.next() {
            // there is an update in the future
            Some(IterItem { offset, .. }) => now_minus_1 + offset,
            // there is no update in the future
            None if self.zeroes_max == 0 => u64::MAX,
            // another update at this frame is expected
            None => now_minus_1,
        }
    }

    /// Shorten the log when log exits or clears were missed.
    ///
    /// If this method returns `true`, call [`Self::clear`] or do comparable operations.
    #[track_caller]
    fn pre_update_to_clear(&mut self, meta: &RevMeta) -> bool {
        match meta.set_update_state(&mut self.update_state) {
            PreUpdateKind::RemoveLog => true,
            PreUpdateKind::RemoveFuture => {
                if self.offset_bytes.len() > self.index {
                    self.offset_bytes.truncate(self.index);
                    self.zeroes = 0;
                }
                self.zeroes_max = self.zeroes;
                false
            }
            PreUpdateKind::Nothing => false,
        }
    }

    /// Clear the log to be in the state of construction except of [`Self::update_state`] which is
    /// not reset.
    fn clear(&mut self) {
        self.offset_bytes.clear();
        self.log_start = 0;
        self.last_update = 0;
        self.index = 0;
        self.past_len = 0;
        self.zeroes = 0;
        self.zeroes_max = 0;
    }
}

/// Defines in which way a log has to be adjusted to reflect new changes to
/// [`RevMeta`](crate::meta::RevMeta) since the last time the log was updated.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    use crate::meta::{RevDirection, RevQueue};

    use super::*;

    struct MetaAndLog {
        meta: RevMeta,
        update_log: UpdateLog,
        last_update: MaybeLocation,
    }

    impl MetaAndLog {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(core::num::NonZeroU64::new(max_world_states), false),
                update_log: UpdateLog::new(),
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
                for past_len in past_lens {
                    let actual = self.update_log.forward_past_len_with_caller(meta, caller);
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
                    self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        for _ in 0..insufficient_updates {
                            assert_eq!(self.update_log.forward_log_with_caller(meta, caller), true);
                        }
                    });
                    self.revert(insufficient_updates, false);
                }
            }

            // test where all updates ran
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::FORWARD_LOG);
                for _ in 0..updates {
                    assert!(self.update_log.forward_log_with_caller(meta, caller));
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
                    self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        for _ in 0..insufficient_updates {
                            assert_eq!(
                                self.update_log.backward_log_with_caller(meta, caller),
                                true
                            );
                        }
                    });
                    self.revert(insufficient_updates, true);
                }
            }

            // test where all updates ran
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::BackwardLog);
                for _ in 0..updates {
                    assert!(self.update_log.backward_log_with_caller(meta, caller));
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
                RevQueue::RUN_FORWARD_LOG
            } else {
                RevQueue::RUN_BACKWARD_LOG
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, _| {
                if forward {
                    for _ in 0..updates {
                        assert_eq!(
                            self.update_log
                                .forward_log_with_caller(meta, self.last_update),
                            true,
                        );
                    }
                } else {
                    for _ in 0..updates {
                        assert_eq!(
                            self.update_log
                                .backward_log_with_caller(meta, self.last_update),
                            true,
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut meta_and_log = MetaAndLog::new(4);

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
    fn behaves_like_meta_minus_gaps() {
        let mut meta_and_log = MetaAndLog::new(4);

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
}
