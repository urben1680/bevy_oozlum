use alloc::{
    boxed::Box,
    collections::{
        TryReserveError, VecDeque,
        vec_deque::{Drain, Iter, IterMut},
    },
};
use core::{fmt::Debug, mem::ManuallyDrop, num::NonZeroU64};

use crate::{
    log::{DrainAll, GapRange, OutOfLog, prepend},
    meta::RevMeta,
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
/// Depending on the direction, either a new log entry is pushed into the log or it is traversed
/// forwards or backwards, yielding mutable log entry references.
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
///         RevDirection::NotLog(not_log) => {
///             let new_transition: MyTransition = todo!();
///
///             // mutate some state with the new transition
///
///             // push transition to the log
///             let mut drain = log.forward_push(&meta, not_log, new_transition);
///
///             // optional, iterate log entries that are now out-of-log
///             for old_transition in drain.past() {
///                 // clean-up logic
///             }
///             // `drain.future()` or `drain.all()` are also available
///         },
///         RevDirection::ForwardLog => {
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
    /// [`Self::witnessed_log_exits`] is outdated. This way the draining of future transitions that
    /// got out-of-log can be postponed to the next [`Self::forward_push`] call.
    index_max: usize,

    /// Contains the most recent global count of log exits that was witnessed.
    ///
    /// See [`RevMeta::log_exits`].
    witnessed_log_exits: u64,

    /// Contains the most recent global count of log clears that was witnessed.
    ///
    /// See [`RevMeta::log_clears`].
    witnessed_log_clears: u64,
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
            witnessed_log_exits: 0,
            witnessed_log_clears: 0,
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

    /// Returns an iterator of the stored log entries.
    ///
    /// See [`VecDeque::iter`].
    pub fn iter<'a>(&'a self) -> Iter<'a, T> {
        self.transitions.iter()
    }

    /// Returns an iterator of the stored log entries.
    ///
    /// See [`VecDeque::iter_mut`].
    pub fn iter_mut<'a>(&'a mut self) -> IterMut<'a, T> {
        self.transitions.iter_mut()
    }

    /// Updates the log with a new `transition` and returns [`TransitionDrain`] that can be used to
    /// iterate log entries that got out-of-log with this push.
    ///
    /// This is used during [`RevDirection::NotLog`](crate::meta::RevDirection::NotLog). Its
    /// field, [`NotLog`](crate::meta::NotLog), can be used for the `past_len` parameter
    /// here if this log is updated exactly once per frame. Otherwise, use
    /// [`UpdateLog`](super::UpdateLog::forward_past_len) instead.
    ///
    /// For an example, see the [type level docs](TransitionLog).
    pub fn forward_push<'a>(
        &'a mut self,
        meta: &RevMeta,
        past_len: impl Into<NonZeroU64>,
        transition: T,
    ) -> TransitionDrain<'a, T> {
        let gap_range = if self.witnessed_log_clears < meta.log_clears() {
            self.witnessed_log_clears = meta.log_clears();
            GapRange::new_clear(self.index)
        } else {
            let past_len = past_len.into().get().try_into().unwrap_or(usize::MAX);
            GapRange::new(self.index.saturating_sub(past_len - 1), self.index)
        };
        self.witnessed_log_exits = meta.log_exits();
        TransitionDrain {
            log: self,
            transition: ManuallyDrop::new(transition),
            gap_range,
            gap_buffer: Default::default(),
        }
    }

    /// Returns a reference to the log entry that was logged at the chronologically previous push.
    /// If the log is at the past end before this call, this method returns an [`OutOfLog`] error,
    /// leaving the log unchanged.
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
        if self.witnessed_log_clears >= meta.log_clears()
            && let Some(index) = self.index.checked_sub(1)
        {
            if self.witnessed_log_exits < meta.log_exits() {
                self.witnessed_log_exits = meta.log_exits();
                self.index_max = self.index;
            }
            // self.index should always be <= the deque len, so successfully reducing the index
            // without underflow is expected to result in a valid index into the log.
            let transition = self.transitions.get_mut(index).unwrap();
            self.index = index;
            Ok(transition)
        } else {
            Err(OutOfLog::caller())
        }
    }

    /// Returns a reference to the log entry that was logged at the chronologically next push. If
    /// the log is at the future end before this call, this method returns an [`OutOfLog`] error,
    /// leaving the log unchanged.
    ///
    /// The log entry can be mutated in case applying it is not only changing the state but also the
    /// log entry itself. This may be needed if a previously added value is taken again
    /// and stored in this log entry at [`backward_log`](Self::backward_log). `forward_log` would
    /// then take the value from the log entry to return it.
    ///
    /// This is used during [`RevDirection::ForwardLog`](crate::meta::RevDirection::ForwardLog).
    ///
    /// For an example, see the [type level docs](TransitionLog).
    #[track_caller]
    pub fn forward_log<'a>(&'a mut self, meta: &RevMeta) -> Result<&'a mut T, OutOfLog> {
        if self.witnessed_log_clears < meta.log_clears()
            || self.witnessed_log_exits < meta.log_exits()
            || self.index >= self.index_max
        {
            return Err(OutOfLog::caller());
        }

        // should not panic: self.transitions.len() >= self.index_max > self.index
        let transition = self.transitions.get_mut(self.index).unwrap();
        self.index += 1;
        Ok(transition)
    }
}

/// A container returned by [`TransitionLog::forward_push`] that can be used to iterate the log
/// entries that are to be truncated because they are out of log now.
///
/// The content of the available drains look like this:
///
/// The letters are all stored log entries with the number below indicating how many updates ago the
/// entry was pushed. Positive numbers are in the future, which is the case after
/// [`TransitionLog::backward_log`] was used three times. After the drains are performed, the actual
/// new entry `X` is pushed.
///
/// ```text
///  self.past       +      self.future  =  self.all
/// |         |             |         |
/// [A] [B] [C] [D] [E] [F] [G] [H] [I]
/// -5  -4  -3  -2  -1   0   1   2   3
///
///               after  drop
///             [D] [E] [F] [X]
///             -3  -2   1   0
/// ```
///
/// The `past_len` value would be `4` in this example:
/// - 4 past states need 3 log entries to transition between them; `D`, `E` and `F`
/// - `X` is the log entry to transition into the present state
#[derive(Debug)]
pub struct TransitionDrain<'a, T> {
    log: &'a mut TransitionLog<T>,
    transition: ManuallyDrop<T>,
    gap_range: GapRange,
    gap_buffer: Box<[T]>,
}

impl<'a, T> TransitionDrain<'a, T> {
    /// Returns log entries that were pushed before [`RevMeta::past_end`].
    pub fn past<'b>(&'b mut self) -> Drain<'b, T> {
        let end = self.gap_range.take_drain_past_end();
        self.log.transitions.drain(..end)
    }

    /// Returns log entries that were pushed after [`RevMeta::now`] which, at this point of time,
    /// is equal to [`RevMeta::future_end`].
    pub fn future<'b>(&'b mut self) -> Drain<'b, T> {
        let start = self.gap_range.drain_future_start();
        self.log.transitions.drain(start..)
    }

    /// Returns log entries that were pushed before [`RevMeta::past_end`] or after [`RevMeta::now`]
    /// which, at this point of time, is equal to [`RevMeta::future_end`].
    pub fn all<'b>(&'b mut self) -> DrainAll<'b, T> {
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

    pub(super) fn iter_past<'b>(&'b self) -> Iter<'b, T> {
        self.log.transitions.range(..self.gap_range.start)
    }

    pub(super) fn transition_mut(&mut self) -> &mut T {
        &mut self.transition
    }
}

impl<T> Drop for TransitionDrain<'_, T> {
    fn drop(&mut self) {
        if self.gap_range.is_clear() {
            self.log.transitions.clear();
        } else {
            self.log.transitions.truncate(self.gap_range.end);
            // todo: use truncate_front https://github.com/rust-lang/rust/issues/140667
            self.log.transitions.drain(..self.gap_range.start);
        }
        prepend(&mut self.log.transitions, &mut self.gap_buffer);
        let transition = unsafe {
            // SAFETY: Only called this once and only in this Drop
            ManuallyDrop::take(&mut self.transition)
        };
        self.log.transitions.push_back(transition);
        self.log.index = self.log.transitions.len();
        self.log.index_max = self.log.transitions.len();
    }
}

#[cfg(test)]
mod test {
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
        fn new(max_past_len: u64) -> Self {
            Self {
                meta: RevMeta::new(max_past_len, false),
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
                RevQueue::ClearThenRunForward
            } else {
                RevQueue::RunForward
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, direction| {
                let RevDirection::NotLog(not_log) = direction else {
                    unreachable!()
                };
                self.logs.assert_forward_transition(
                    meta,
                    not_log,
                    &past_drain,
                    &future_drain,
                    push,
                );
            });
        }
        fn noop_forward_backward_log(&mut self) {
            self.meta.set_queue(RevQueue::RunForward);
            self.meta.update_ref(Ok(true), |_, _| ());
            self.meta.set_queue(RevQueue::RunBackwardLog);
            self.meta.update_ref(Ok(true), |_, _| ());
        }
        #[track_caller]
        fn forward_log(&mut self, expected: Result<char, ()>) {
            self.meta.set_queue(RevQueue::RunForwardLog);
            match expected {
                Ok(_) => {
                    self.meta.update_ref(Ok(true), |meta, _| {
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
            self.meta.update_ref(Ok(true), |meta, _| {
                self.logs.assert_forward_log_transition(meta, Err(()));
            });
        }
        #[track_caller]
        fn backward_log(&mut self, expected: Result<char, ()>) {
            self.meta.set_queue(RevQueue::RunBackwardLog);
            match expected {
                Ok(_) => {
                    self.meta.update_ref(Ok(true), |meta, _| {
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
        let mut meta_and_logs = MetaAndLogs::new(4);
        meta_and_logs.forward([], [], 'a', false);
        meta_and_logs.forward([], [], 'b', false);
        meta_and_logs.forward([], [], 'c', false);
        meta_and_logs.forward([], [], 'd', false);
        meta_and_logs.forward(['a'], [], 'e', false);
        meta_and_logs.forward(['b'], [], 'f', false);

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

        meta_and_logs.forward(['c'], ['d', 'g'], 'h', true);

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

        meta_and_logs.meta.set_max_past_len(1);
        meta_and_logs.forward(['j', 'k'], ['l'], 'm', false);
    }
}
