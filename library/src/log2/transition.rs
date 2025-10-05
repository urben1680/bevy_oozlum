use crate::{
    log2::{DrainAll, GapBuffer, GapRange, OutOfLog, PreUpdateKind},
    meta::{RevDirection, RevMeta},
};
use bevy_log::error;
use core::{
    any::type_name,
    fmt::Debug,
    iter::{FusedIterator, Rev, Skip, Take},
    mem::take,
    ops::Range,
};
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
/// # Examples
///
/// Generally, [`pre_update`](Self::pre_update) or [`pre_update_drain`](Self::pre_update_drain)
/// need to be called before any other mutation. These methods handle when
/// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was queued and applied or the
/// [`RevDirection`](crate::meta::RevDirection) changed from a
/// [log variant](crate::meta::RevDirection::is_log) to the
/// [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG) one.
///
/// These methods make sure the log catches such changes even if the current system does not run in
/// the very same frame they took effect, like when that frame was missed because of run conditions.
///
/// If the log is mutated multiple times per frame, these `pre_update` methods only need to called
/// once at the beginning.
///
/// After that, depending on the direction, either a new transition is pushed into the log or it is
/// traversed forwards or backwards, yielding a transition reference.
///
/// This log alone is only suited for a constant amount of updates per frame. For a variable amount
/// of updates, like when the system is skipped completely sometimes, consider pairing it with a
/// [`PastLenLog`](crate::log::PastLenLog).
///
/// ## Basic `Local` usage
///
/// Usually transition types are just plain data the log can truncate when it is no longer needed.
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
///     log.pre_update(&meta);
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///
///             // mutate some state with the new transition
///
///             // push transition to the log
///             log.push(meta.past_len(), new_transition);
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition: MyTransition = log.forward_log()?.clone();
///
///             // mutate some state with the logged transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition: MyTransition = log.backward_log()?.clone();
///
///             // mutate some state with the logged transition
///         }
///     }
///
///     Ok(())
/// }
/// ```
///
/// ## Draining future
///
/// There may be cases where extra cleanup is needed in which case the transitions that the log no
/// longer needs should be drained with in an iterator.
///
/// In this example the cleanup is required for log entries that are in the future part.
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
///     for future_transition in log.pre_update_drain(&meta).future() {
///
///         // do cleanup tasks with future transitions
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///
///             // mutate some state with the new transition
///
///             // push the transition to the log
///             log.push(meta.past_len(), new_transition);
///         },
///         RevDirection::FORWARD_LOG => todo!(), // same as first example
///         RevDirection::BackwardLog => todo!() // same as first example
///     }
///
///     Ok(())
/// }
/// ```
///
/// ## Draining past
///
/// Like the _Draining future_ example, but here the relevant transitions for cleanup work are in
/// the past part of the log.
///
/// There are two iterations in the example where cleanup work has to happen. This is because either
/// of the two could happen in isolation, the first because
/// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was queued and applied and the second because
/// the log exceeds [`RevMeta::past_len`].
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
///     for past_transition in log.pre_update_drain(&meta).past() {
///
///         // do cleanup tasks with past transitions
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///
///             // mutate some state with the new transition
///
///             // push the transition to the log
///             let drain = log.push_drain_past(meta.past_len(), new_transition);
///
///             for past_transition in drain {
///
///                 // do cleanup tasks with past transitions
///             }
///         },
///         RevDirection::FORWARD_LOG => todo!(), // same as first example
///         RevDirection::BackwardLog => todo!() // same as first example
///     }
///
///     Ok(())
/// }
/// ```
///
/// ## Draining future and past
///
/// Combination of the two examples above. The distinction between future and past transitions is
/// optional; instead of [`past`](TransitionDrains::past) and [`future`](TransitionDrains::future),
/// [`all`](TransitionDrains::all) can be used.
///
/// The `MyTransition` may for example contain an [entity ID](bevy_ecs::entity::Entity). This could be
/// the ID of a temporal entity that is associated to this transition. The cleanup then would be to
/// despawn it.
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
///     let mut drains = log.pre_update_drain(&meta);
///
///     for past_transition in drains.past() {
///
///         // do cleanup tasks with past transitions
///     }
///
///     for future_transition in drains.future() {
///
///         // do cleanup tasks with future transitions
///     }
///
///     // or, instead of the two above: `for transition in drain.all() { ... }`
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///
///             // mutate some state with the new transition
///
///             // push the transition to the log
///             let drain = log.push_drain_past(meta.past_len(), new_transition);
///
///             for past_transition in drain {
///
///                 // do cleanup tasks with past transitions
///             }
///         },
///         RevDirection::FORWARD_LOG => todo!(), // same as first example
///         RevDirection::BackwardLog => todo!() // same as first example
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

    index_max: usize,

    /// Contains the most recent global count of log exits that was witnessed.
    ///
    /// See [`RevMeta::log_exits`].
    meta_log_exits: u64,

    /// Contains the most recent global count of log clears that was witnessed.
    ///
    /// See [`RevMeta::log_clears`].
    meta_log_clears: u64,

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
    /// If present, this has an effect on the following methods:
    ///
    /// - [`pre_update`](Self::pre_update)/[`pre_update_drain`](Self::pre_update_drain) will not
    ///   truncate/drain any log entries except if [`RevQueue::Clear`](crate::meta::RevQueue::Clear)
    ///   is applied, which also clears the poison.
    /// - [`push`](Self::push) ignores the pushed log entry and [logs an error](bevy_log::error).
    /// - [`push_drain_past`](Self::push_drain_past) ignores the pushed log entry,
    ///   [logs an error](bevy_log::error) and always returns an drains no log entries.
    /// - [`forward_log`](Self::forward_log)/[`backward_log`](Self::backward_log) continue to return
    ///   the same [`OutOfLog`] error as this method here does.
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
    /// This happens automatically when
    /// [`pre_update`](Self::pre_update)([`_drain`](Self::pre_update_drain)) applies a previous
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear).
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

    pub fn push(
        &mut self,
        meta: &RevMeta,
        max_past_len: u64,
        transition: T,
    ) -> Result<TransitionDrain<T>, OutOfLog> {
        let gap_range = if self.meta_log_clears < meta.log_clears() {
            self.poison = Ok(());
            GapRange::new_clear(self.index)
        } else if let Err(out_of_log) = self.poison {
            return Err(out_of_log);
        } else {
            let max_past_len = usize::try_from(max_past_len)
                .unwrap_or(usize::MAX)
                .saturating_sub(1);
            GapRange {
                start: self.index.saturating_sub(max_past_len),
                start_offset: (),
                end: self.index,
            }
        };
        self.meta_log_clears = meta.log_clears();
        self.meta_log_exits = meta.log_exits();
        Ok(TransitionDrain {
            log: self,
            transition: ManuallyDrop::new(transition),
            push: max_past_len > 0,
            gap_range,
            gap_buffer: GapBuffer::default(),
        })
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
    /// Before calling this, [`pre_update`](Self::pre_update) or
    /// [`pre_update_drain`](Self::pre_update_drain) **must** be called at least once in the
    /// [present reversible frame](RevMeta::now).
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), this always returns an error. If bevy's
    /// `track_location` cargo feature is activated, the error here contains the location where it
    /// originally occured.
    #[track_caller]
    pub fn backward_log(&mut self, meta: &RevMeta) -> Result<&mut T, OutOfLog> {
        self.poison?;

        if self.meta_log_clears >= meta.log_clears()
            && let Some(index) = self.index.checked_sub(1)
        {
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
    /// leaving the log unchanged.
    ///
    /// The log entry can be mutated in case applying it is not only changing the state but also the
    /// log entry itself. This may be needed if a previously added value is taken again
    /// and stored in this log entry at [`backward_log`](Self::backward_log). `forward_log` would
    /// then take the value from the log entry to return it.
    ///
    /// This is used during [`RevDirection::FORWARD_LOG`](crate::meta::RevDirection::FORWARD_LOG).
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) or
    /// [`pre_update_drain`](Self::pre_update_drain) **must** be called at least once in the
    /// [present reversible frame](RevMeta::now).
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), this always returns an error. If bevy's
    /// `track_location` cargo feature is activated, the error here contains the location where it
    /// originally occured.
    #[track_caller]
    pub fn forward_log<'a, 'm>(&'a mut self, meta: &'m RevMeta) -> Result<&'a mut T, OutOfLog> {
        self.poison?;

        if self.meta_log_clears < meta.log_clears()
            || self.meta_log_exits < meta.log_exits()
            || self.index >= self.index_max.min(self.transitions.len())
        {
            return Err(self.set_poison());
        }

        let transition = unsafe {
            // SAFETY: self.index >= self.transitions.len() returned before
            self.transitions.get_mut(self.index).unwrap_unchecked()
        };
        self.index += 1;
        Ok(transition)
    }

    #[track_caller]
    fn set_poison(&mut self) -> OutOfLog {
        let out_of_log = OutOfLog::caller();
        self.poison = Err(out_of_log);
        out_of_log
    }
}

#[derive(Debug)]
pub struct TransitionDrain<'a, T> {
    log: &'a mut TransitionLog<T>,
    transition: ManuallyDrop<T>,
    push: bool,
    gap_range: GapRange<()>,
    gap_buffer: GapBuffer<T>,
}

impl<'a, T> TransitionDrain<'a, T> {
    pub fn drain_past(&mut self) -> Drain<'_, T> {
        self.push = true;
        let end = self.gap_range.drain_past_end();
        self.log.transitions.drain(..end)
    }

    pub fn drain_future(&mut self) -> Drain<'_, T> {
        let start = self.gap_range.drain_future_start();
        self.log.transitions.drain(start..)
    }

    pub fn drain_all(&mut self) -> DrainAll<'_, T, ()> {
        self.push = true;
        DrainAll::new(
            &mut self.log.transitions,
            &mut self.gap_range,
            &mut self.gap_buffer,
        )
    }

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
        if self.gap_range.is_clear() || self.gap_range.start == self.gap_range.end {
            self.log.transitions.clear();
        } else {
            self.log.transitions.truncate(self.gap_range.end);
            // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
            self.log.transitions.drain(..self.gap_range.start);
        }
        self.gap_buffer.prepend_in(&mut self.log.transitions);
        let transition = unsafe {
            // SAFETY: only called this once in Drop
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

    use crate::meta::RevQueue;

    use super::*;

    #[derive(Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        drop_drain: TransitionLog<char>,
        past_drain: TransitionLog<char>,
        future_drain: TransitionLog<char>,
        past_future_drain: TransitionLog<char>,
        future_past_drain: TransitionLog<char>,
        all_drain: TransitionLog<char>,
        past_all_drain: TransitionLog<char>,
        future_all_drain: TransitionLog<char>,
    }

    impl MetaAndLogs {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(NonZeroU64::new(max_world_states), false),
                drop_drain: TransitionLog::new(),
                past_drain: TransitionLog::new(),
                future_drain: TransitionLog::new(),
                past_future_drain: TransitionLog::new(),
                future_past_drain: TransitionLog::new(),
                all_drain: TransitionLog::new(),
                past_all_drain: TransitionLog::new(),
                future_all_drain: TransitionLog::new(),
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

                self.drop_drain.push(meta, meta.past_len(), push).unwrap();

                let mut drain = self.past_drain.push(meta, meta.past_len(), push).unwrap();
                let actual = drain.drain_past();
                assert_eq!(actual.len(), N);
                let actual: Vec<char> = actual.collect();
                assert_eq!(actual, past_drain);
                let actual = drain.drain_past();
                assert_eq!(actual.len(), 0);
                assert_eq!(actual.count(), 0);
                // todo: assert len and count at others too

                let mut drain = self.future_drain.push(meta, meta.past_len(), push).unwrap();
                let actual: Vec<char> = drain.drain_future().collect();
                assert_eq!(actual, future_drain);
                assert_eq!(drain.drain_future().count(), 0);

                let mut drain = self
                    .past_future_drain
                    .push(meta, meta.past_len(), push)
                    .unwrap();
                let actual: Vec<char> = drain.drain_past().collect();
                assert_eq!(actual, past_drain);
                assert_eq!(drain.drain_past().count(), 0);
                let actual: Vec<char> = drain.drain_future().collect();
                assert_eq!(actual, future_drain);
                assert_eq!(drain.drain_future().count(), 0);
                assert_eq!(drain.drain_all().count(), 0);

                let mut drain = self
                    .future_past_drain
                    .push(meta, meta.past_len(), push)
                    .unwrap();
                let actual: Vec<char> = drain.drain_future().collect();
                assert_eq!(actual, future_drain);
                assert_eq!(drain.drain_future().count(), 0);
                let actual: Vec<char> = drain.drain_past().collect();
                assert_eq!(actual, past_drain);
                assert_eq!(drain.drain_past().count(), 0);
                assert_eq!(drain.drain_all().count(), 0);

                let mut drain = self.all_drain.push(meta, meta.past_len(), push).unwrap();
                let actual: Vec<char> = drain.drain_all().collect();
                let expected: Vec<char> = past_drain.into_iter().chain(future_drain).collect();
                assert_eq!(actual, expected);
                assert_eq!(drain.drain_future().count(), 0);

                let mut drain = self
                    .past_all_drain
                    .push(meta, meta.past_len(), push)
                    .unwrap();
                let actual: Vec<char> = drain.drain_past().collect();
                assert_eq!(actual, past_drain);
                assert_eq!(drain.drain_past().count(), 0);
                let actual: Vec<char> = drain.drain_all().collect();
                assert_eq!(actual, future_drain);
                assert_eq!(drain.drain_future().count(), 0);
                assert_eq!(drain.drain_all().count(), 0);

                let mut drain = self
                    .future_all_drain
                    .push(meta, meta.past_len(), push)
                    .unwrap();
                let actual: Vec<char> = drain.drain_future().collect();
                assert_eq!(actual, future_drain);
                assert_eq!(drain.drain_future().count(), 0);
                let actual: Vec<char> = drain.drain_all().collect();
                assert_eq!(actual, past_drain);
                assert_eq!(drain.drain_past().count(), 0);
                assert_eq!(drain.drain_future().count(), 0);
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
            let logs = [
                &mut self.drop_drain,
                &mut self.past_drain,
                &mut self.future_drain,
                &mut self.past_future_drain,
                &mut self.future_past_drain,
                &mut self.all_drain,
                &mut self.past_all_drain,
                &mut self.future_all_drain,
            ];
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            match expected {
                Ok(expected) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);

                        for log in logs {
                            let actual = log.forward_log(meta).map(|char| *char);
                            assert_eq!(actual, Ok(expected));
                        }
                    });
                }
                Err(()) => {
                    self.meta.update_ref(Ok(false), |_, _| ());

                    for log in logs {
                        assert_eq!(log.forward_log(&self.meta), Err(OutOfLog::caller()));
                        log.clear_poison();
                    }
                }
            }
        }
        #[track_caller]
        fn backward_log(&mut self, expected: Result<char, ()>) {
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            match expected {
                Ok(expected) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);

                        for log in [
                            &mut self.drop_drain,
                            &mut self.past_drain,
                            &mut self.future_drain,
                            &mut self.past_future_drain,
                            &mut self.future_past_drain,
                            &mut self.all_drain,
                            &mut self.past_all_drain,
                            &mut self.future_all_drain,
                        ] {
                            let actual = log.backward_log(meta).map(|char| *char);
                            assert_eq!(actual, Ok(expected));
                        }
                    });
                }
                Err(()) => {
                    self.meta.update_ref(Ok(false), |_, _| ());

                    for log in [&mut self.drop_drain, &mut self.future_drain] {
                        assert_eq!(log.backward_log(&self.meta), Err(OutOfLog::caller()));
                        log.clear_poison();
                    }

                    for (i, log) in [
                        &mut self.past_drain,
                        &mut self.past_future_drain,
                        &mut self.future_past_drain,
                        &mut self.all_drain,
                        &mut self.past_all_drain,
                        &mut self.future_all_drain,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        match log.backward_log(&self.meta) {
                            Ok(expected) => {
                                let expected = *expected;
                                assert_eq!(
                                    log.backward_log(&self.meta),
                                    Err(OutOfLog::caller()),
                                    "{i}"
                                );
                                log.clear_poison();

                                // undo Ok
                                let actual = log.forward_log(&self.meta).map(|char| *char);
                                assert_eq!(actual, Ok(expected), "{i}");
                            }
                            Err(out_of_log) => {
                                assert_eq!(out_of_log, OutOfLog::caller(), "{i}");
                                log.clear_poison();
                            }
                        }
                    }
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
