use crate::{
    log::{DrainAll, GapRange, OutOfLog, prepend},
    meta::RevMeta,
};
use core::fmt::Debug;
use std::{
    collections::{
        TryReserveError, VecDeque,
        vec_deque::{Drain, Iter},
    },
    mem::ManuallyDrop,
    usize,
};

/// A log that is updated with exactly one transition type `T` that is used to transition a state
/// forward or backward in time.
///
/// This log alone is only suited for a constant amount of updates per frame. For a variable amount
/// of updates, like when the system is skipped completely sometimes, consider pairing it with an
/// [`UpdateLog`](crate::log::UpdateLog).
///
/// # Example
///
/// Depending on the direction, either a new transition is pushed into the log or it is traversed
/// forwards or backwards, yielding a mutable transition reference.
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # #[derive(Clone)]
/// # struct MyTransition;
/// fn system(
///     meta: Res<RevMeta>,
///     mut log: Local<TransitionLog<MyTransition>>
/// ) -> Result<(), BevyError> {
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///
///             // mutate some state with the new transition
///
///             // push transition to the log
///             let mut drain = log.push(&meta, meta.past_len(), new_transition)?;
///
///             // optional, iterate log entries that are now out-of-log
///             for old_transition in drain.past() {
///                 // clean-up logic
///             }
///             // `drain.future()` or `drain.all()` are also available
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition: MyTransition = log.forward_log(&meta)?.clone();
///
///             // mutate some state with the logged transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition: MyTransition = log.backward_log(&meta)?.clone();
///
///             // mutate some state with the logged transition
///         }
///     }
///
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct TransitionLog<T> {
    /// Contains the transition values in the order they were pushed.
    transitions: VecDeque<T>,

    /// Points to the chronologically next transition in [`Self::transitions`]. If it is equal to
    /// the length of it, the log reached its future end.
    index: usize,

    /// The maximum value for [`Self::index`]. Is usually equal to `transitions.len()` but can be
    /// lower when this log is updated during
    /// [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) and
    /// [`Self::meta_log_exits`] is outdated. This way the draining of future transitions that got
    /// out-of-log can be postponed to the next [`Self::push`] call.
    index_max: usize,

    /// Contains the most recent global count of log exits that was witnessed.
    ///
    /// See [`RevMeta::log_exits`].
    meta_log_exits: u64,

    /// Contains the most recent global count of log clears that was witnessed.
    ///
    /// See [`RevMeta::log_clears`].
    meta_log_clears: u64,

    /// The last error of [`Self::backward_log`]/[`Self::forward_log`].
    poison: Result<(), OutOfLog>,
}

impl<T> Default for TransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> TransitionLog<T> {
    /// Creates an empty log.
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            index: 0,
            index_max: 0,
            meta_log_exits: 0,
            meta_log_clears: 0,
            poison: Ok(()),
        }
    }

    /// Creates an empty log with space for at least `capacity` transitions.
    ///
    /// See [`VecDeque::with_capacity`].
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(capacity),
            ..Self::new()
        }
    }

    /// Returns the current poison state.
    ///
    /// When a log is poisoned, then the stored log entries are considered out-of-sync with the
    /// global log that is tracked by [`RevMeta`].
    ///
    /// If present, this has an effect on the following methods:
    ///
    /// - [`push`](Self::push) ignores the pushed log entry and will not truncate/drain any log
    ///   entries and [logs an error](bevy_log::error). If
    ///   [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was applied previously, then the poison
    ///   will be cleared and `push` works again.
    /// - [`forward_log`](Self::forward_log)/[`backward_log`](Self::backward_log) continue to return
    ///   the same [`OutOfLog`] error and will not traverse the log, even if it would be possible
    ///   otherwise.
    ///
    /// If bevy's `track_location` cargo feature is activated, the error here contains the location
    /// where it originally occured.
    ///
    /// To unset the poison, [`clear_poison`](Self::clear_poison) can be called.
    pub fn poison(&self) -> Result<(), OutOfLog> {
        self.poison
    }

    /// Unsets the poison, see [`poison`](Self::poison).
    ///
    /// This should only be done if it is sure this leaves the log in a valid state that is in sync
    /// with the global log.
    ///
    /// This happens automatically when [`push`](Self::push) applies a previous
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) as that removes all log entries so far
    /// that could be out-of-sync.
    pub fn clear_poison(&mut self) {
        self.poison = Ok(());
    }

    /// Returns the number of transitions in the log.
    ///
    /// See [`VecDeque::len`].
    pub fn len(&self) -> usize {
        self.transitions.len()
    }

    /// Returns the number of transitions the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`].
    pub fn capacity(&self) -> usize {
        self.transitions.capacity()
    }

    /// Returns `true` if the log contains no transitions.
    ///
    /// See [`VecDeque::is_empty`].
    pub fn is_empty(&self) -> bool {
        self.transitions.is_empty()
    }

    /// Reserves capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::reserve`].
    pub fn reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }

    /// Reserves capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::reserve_exact`].
    pub fn reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::try_reserve`].
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::try_reserve_exact`].
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    pub fn shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }

    /// Updates the log with a new `transition` and returns [`TransitionDrain`] that can be used to
    /// iterate log entries that got out-of-log with this push.
    ///
    /// If [`backward_log`](Self::backward_log)/[`forward_log`](Self::forward_log) returned an
    /// [`OutOfLog`] error previously, the same error will be returned here as well and `transition`
    /// will not be pushed.
    ///
    /// If [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was applied in the meantime, any
    /// previous error will be cleared and this log is mutable again. Alternatively
    /// [`Self::clear_poison`] can be used to undo the error.
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    ///
    /// For an example, see the [type level docs](TransitionLog).
    pub fn push<'a>(
        &'a mut self,
        meta: &RevMeta,
        max_past_len: u64,
        transition: T,
    ) -> Result<TransitionDrain<'a, T>, OutOfLog> {
        let gap_range = if self.meta_log_clears < meta.log_clears() {
            self.meta_log_clears = meta.log_clears();
            self.poison = Ok(());
            GapRange::new_clear(self.index)
        } else if let Err(out_of_log) = self.poison {
            return Err(out_of_log);
        } else {
            let max_past_len = usize::try_from(max_past_len)
                .unwrap_or(usize::MAX)
                .saturating_sub(1);
            GapRange::new_offset_one(self.index.saturating_sub(max_past_len), self.index)
        };
        self.meta_log_exits = meta.log_exits();
        Ok(TransitionDrain {
            log: self,
            transition: ManuallyDrop::new(transition),
            push: max_past_len > 0,
            gap_range,
            gap_buffer: Default::default(),
        })
    }

    /// Returns a reference to the log entry that was logged at the chronologically previous push.
    /// If the log is at the past end before this call, this method returns an [`OutOfLog`] error,
    /// leaving the log unchanged. The same is true if a previous error was not cleared yet
    /// [manually](Self::clear_poison) or by an applied
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) followed by a [`push`](Self::push) call.
    ///
    /// The log entry can be mutated in case applying it is not only changing the state but also the
    /// log entry itself. This may be needed if a previously added value is taken again and stored
    /// in this log entry at `backward_log`. [`forward_log`](Self::forward_log) would then take the
    /// value from the log entry to return it.
    ///
    /// This is used during [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog).
    ///
    /// For an example, see the [type level docs](TransitionLog).
    #[track_caller]
    pub fn backward_log(&mut self, meta: &RevMeta) -> Result<&mut T, OutOfLog> {
        self.poison?;

        if self.meta_log_clears >= meta.log_clears()
            && let Some(index) = self.index.checked_sub(1)
        {
            if self.meta_log_exits < meta.log_exits() {
                self.meta_log_exits = meta.log_exits();
                self.index_max = self.index;
            }
            // self.index should always be <= the deque len, so successfully reducing the index
            // without underflow is expected to result in a valid index into the log. If this is
            // not the case here, this would be a crate bug.
            let transition = self.transitions.get_mut(index).unwrap();
            self.index = index;
            Ok(transition)
        } else {
            Err(self.set_poison())
        }
    }

    /// Returns a reference to the log entry that was logged at the chronologically next push. If
    /// the log is at the future end before this call, this method returns an [`OutOfLog`] error,
    /// leaving the log unchanged. The same is true if a previous error was not cleared yet
    /// [manually](Self::clear_poison) or by an applied
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) followed by a [`push`](Self::push) call.
    ///
    /// The log entry can be mutated in case applying it is not only changing the state but also the
    /// log entry itself. This may be needed if a previously added value is taken again
    /// and stored in this log entry at [`backward_log`](Self::backward_log). `forward_log` would
    /// then take the value from the log entry to return it.
    ///
    /// This is used during [`RevDirection::FORWARD_LOG`](crate::meta::RevDirection::FORWARD_LOG).
    ///
    /// For an example, see the [type level docs](TransitionLog).
    #[track_caller]
    pub fn forward_log<'a, 'm>(&'a mut self, meta: &'m RevMeta) -> Result<&'a mut T, OutOfLog> {
        self.poison?;

        if self.meta_log_clears < meta.log_clears()
            || self.meta_log_exits < meta.log_exits()
            || self.index >= self.index_max
        {
            return Err(self.set_poison());
        }

        // should not panic: self.transitions.len() >= self.index_max > self.index
        let transition = self.transitions.get_mut(self.index).unwrap();
        self.index += 1;
        Ok(transition)
    }

    /// Set and return [`OutOfLog`] with the current caller.
    #[track_caller]
    fn set_poison(&mut self) -> OutOfLog {
        let out_of_log = OutOfLog::caller();
        self.poison = Err(out_of_log);
        out_of_log
    }
}

/// A container returned by [`TransitionLog::push`] that can be used to iterate the log entried that
/// are to be truncated because they are out of log.
///
/// The content of the available drains look like this:
///
/// The letters are all stored log entries with the number below indicating how many updates ago the
/// entry was pushed. Positive numbers are in the future, which is the case after
/// [`TransitionLog::backward_log`] was used three times. When the drains are performed, the actual
/// new entry `X` is pushed.
///
/// The `max_past_len` value would be `3` in this example.
///
/// ```text
/// [A] [B] [C] [D] [E] [F] [G] [H] [I]
/// -5  -4  -3  -2  -1   0   1   2   3
/// |_________|             |_________|
/// past drain              future drain
///             [D] [E] [F] [X]
///             -3  -2   1   0
/// ```
///
/// Note that `D` is actually not needed for this log anymore but may still be kept:
///
/// A log entry is used to transition between two states. Because of this, `N` log entries are
/// needed for `N+1` states. If `max_past_len` is now `3`, that means plus the present state there
/// are 4 global states. Transitioning between them would need only three log entries, but as the
/// above scheme shows, the final amount of log entries with `X` is four.
///
/// The reason is it may lead to subtle bugs in the cleanup logic if `D` was included in the past
/// drain. `D` was pushed at a frame that is still >= [`RevMeta::past_end`] and that may be
/// unexpected. Because of this, `D` is kept if the past is actively drained via
/// [`past`](Self::past) or [`all`](Self::all). If only [`future`](Self::future) is used or this
/// container is dropped unused, `D` will be truncated.
#[derive(Debug)]
pub struct TransitionDrain<'a, T> {
    log: &'a mut TransitionLog<T>,
    transition: ManuallyDrop<T>,
    push: bool,
    gap_range: GapRange,
    gap_buffer: Box<[T]>,
}

impl<'a, T> TransitionDrain<'a, T> {
    /// Returns log entries that were pushed before [`RevMeta::past_end`].
    pub fn past(&mut self) -> Drain<'_, T> {
        self.push = true;
        let end = self.gap_range.drain_past_end();
        self.log.transitions.drain(..end)
    }

    /// Returns log entries that were pushed after [`RevMeta::now`] which, at this point of time,
    /// is equal to [`RevMeta::future_end`].
    pub fn future(&mut self) -> Drain<'_, T> {
        let start = self.gap_range.drain_future_start();
        self.log.transitions.drain(start..)
    }

    /// Returns log entries that were pushed before [`RevMeta::past_end`] or after [`RevMeta::now`]
    /// which, at this point of time, is equal to [`RevMeta::future_end`].
    pub fn all(&mut self) -> DrainAll<'_, T> {
        self.push = true;
        DrainAll::new(
            &mut self.log.transitions,
            &mut self.gap_range,
            &mut self.gap_buffer,
        )
    }

    /// Returns `true` if all log entries are to be cleared, regardless of the user actively drains
    /// them.
    pub(super) fn is_clear(&self) -> bool {
        self.gap_range.is_clear()
    }

    pub(super) fn iter_past(&self) -> Iter<T> {
        self.log.transitions.range(..self.gap_range.start)
    }

    pub(super) fn transition_mut(&mut self) -> &mut T {
        &mut self.transition
    }

    pub(super) fn does_push(&self) -> bool {
        self.push
    }
}

impl<T> Drop for TransitionDrain<'_, T> {
    fn drop(&mut self) {
        if self.gap_range.is_clear() {
            self.log.transitions.clear();
        } else {
            self.log.transitions.truncate(self.gap_range.end);
            // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
            self.log.transitions.drain(..self.gap_range.start);
        }
        prepend(&mut self.gap_buffer, &mut self.log.transitions);
        let transition = unsafe {
            // SAFETY: ManuallyDrop remains unused after this until the end of drop
            ManuallyDrop::take(&mut self.transition)
        };
        if self.push {
            self.log.transitions.push_back(transition);
        }
        self.log.index = self.log.transitions.len();
        self.log.index_max = self.log.transitions.len();
    }
}

#[cfg(test)]
mod test {
    use core::num::NonZeroU64;

    use crate::{
        log::test::Logs,
        meta::{RevDirection, RevQueue},
    };

    use super::*;

    #[derive(Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        logs: Logs<TransitionLog<char>>,
    }

    impl MetaAndLogs {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(NonZeroU64::new(max_world_states), false),
                logs: Logs::default(),
            }
        }
        fn forward<const N: usize, const M: usize>(
            &mut self,
            past_drain: [char; N],
            future_drain: [char; M],
            push: char,
            clear: bool,
        ) {
            let queue = if clear {
                RevQueue::CLEAR_THEN_RUN
            } else {
                RevQueue::RUN_NOT_LOG
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::NOT_LOG);
                self.logs.assert_forward_transition(
                    meta,
                    meta.past_len(),
                    &past_drain,
                    &future_drain,
                    push,
                );
            });
        }
        fn noop_forward_backward_log(&mut self) {
            self.meta.set_queue(RevQueue::RUN_NOT_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
        }
        #[track_caller]
        fn forward_log(&mut self, expected: Result<char, ()>) {
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            match expected {
                Ok(_) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        self.logs.assert_forward_log_transition(meta, expected);
                    });
                }
                Err(()) => {
                    self.meta.update_ref(Ok(false), |_, _| ());
                    self.logs
                        .assert_forward_log_transition(&self.meta, expected);
                }
            }
        }
        fn forward_log_err_in_global_log(&mut self) {
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::FORWARD_LOG);
                self.logs.assert_forward_log_transition(meta, Err(()));
            });
        }
        #[track_caller]
        fn backward_log(&mut self, expected: Result<char, ()>) {
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            match expected {
                Ok(_) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        self.logs.assert_backward_log_transition(meta, expected);
                    });
                }
                Err(()) => {
                    self.meta.update_ref(Ok(false), |_, _| ());
                    self.logs
                        .assert_backward_log_transition(&self.meta, expected);
                }
            }
        }
    }

    #[test]
    fn traverses_log() {
        let mut meta_and_logs = MetaAndLogs::new(5);
        meta_and_logs.forward([], [], 'a', false);
        meta_and_logs.forward([], [], 'b', false);
        meta_and_logs.forward([], [], 'c', false);
        meta_and_logs.forward([], [], 'd', false);
        meta_and_logs.forward([], [], 'e', false); // non-past draining remove 'a' here
        meta_and_logs.forward(['a'], [], 'f', false);

        meta_and_logs.backward_log(Ok('f'));
        meta_and_logs.backward_log(Ok('e'));
        meta_and_logs.backward_log(Ok('d'));
        meta_and_logs.backward_log(Ok('c'));
        meta_and_logs.backward_log(Err(())); // 'b' is unreachable but not yet drained

        meta_and_logs.forward_log(Ok('c'));
        meta_and_logs.forward_log(Ok('d'));
        meta_and_logs.forward_log(Ok('e'));
        meta_and_logs.forward_log(Ok('f'));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log(Ok('f'));
        meta_and_logs.backward_log(Ok('e'));

        meta_and_logs.forward([], ['e', 'f'], 'g', false);

        meta_and_logs.backward_log(Ok('g'));
        meta_and_logs.backward_log(Ok('d'));
        meta_and_logs.backward_log(Ok('c'));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward_log(Ok('c'));
        meta_and_logs.forward_log(Ok('d'));
        meta_and_logs.forward_log(Ok('g'));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log(Ok('g'));
        meta_and_logs.backward_log(Ok('d'));

        meta_and_logs.forward(['b', 'c'], ['d', 'g'], 'h', true);

        meta_and_logs.backward_log(Ok('h'));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward_log(Ok('h'));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.forward([], [], 'i', false);

        meta_and_logs.backward_log(Ok('i'));

        meta_and_logs.noop_forward_backward_log();

        meta_and_logs.backward_log(Ok('h'));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward_log(Ok('h'));
        meta_and_logs.forward_log_err_in_global_log();

        meta_and_logs.backward_log(Ok('h'));

        meta_and_logs.forward([], ['h', 'i'], 'j', false);
        meta_and_logs.forward([], [], 'k', false);
        meta_and_logs.forward([], [], 'l', false);

        meta_and_logs.backward_log(Ok('l'));

        meta_and_logs
            .meta
            .set_max_world_states(NonZeroU64::new(2).unwrap());
        meta_and_logs.forward(['j'], ['l'], 'm', false);
    }
}
