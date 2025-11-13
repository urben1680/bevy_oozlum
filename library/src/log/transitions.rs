use crate::{
    log::{
        DrainAll, GapRange, OutOfLog, TransitionDrain, TransitionLog, prepend,
        transition::DrainAndMaybePushedTransition,
    },
    meta::RevMeta,
};
use core::{
    fmt::Debug,
    iter::{FusedIterator, Take},
    marker::PhantomData,
    mem::ManuallyDrop,
};
use std::collections::{
    TryReserveError, VecDeque,
    vec_deque::{Drain, IterMut},
};

/// A log that is updated with with a variable amount of transition type `T` which are used to
/// transition a state forward or backward in time. For each of these updates, a type `U` is stored
/// along it which is by default `()`.
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
/// After that, depending on the direction, either new transitions are pushed into the log or it is
/// traversed forwards or backwards, yielding transition references.
///
/// This log alone is only suited for one update per frame. For a variable amount
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
/// # #[derive(Clone)]
/// # struct MyUpdate;
/// fn system(
///     meta: Res<RevMeta>,
///     mut log: Local<TransitionsLog<MyTransition, MyUpdate>>
/// ) -> Result<(), BevyError> {
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transitions: Vec<MyTransition> = todo!();
///             let new_update: MyUpdate = todo!();
///
///             // mutate some state with the new transitions and update
///
///             // push transitions and update to the log
///             let mut drain = log.extend_with(
///                 &meta,
///                 meta.past_len(),
///                 new_transitions,
///                 new_update
///             )?;
///
///             // optional, iterate log entries that are now out-of-log
///             
///             let past = drain.past();
///             // if `MyUpdate` is (), `past` is an iterator itself, no need for `past.transitions`
///             for old_transition in past.transitions {
///                 // clean-up logic
///             }
///             for old_update in past.updates {
///                 // clean-up logic
///             }
///
///             let mut future = drain.future();
///             // one can also iterate per log entry
///             while let Some((iter, future_update)) = future.next_log_entry() {
///                 for future_transition in iter {
///
///                     // do cleanup tasks with future transitions and updates
///                 }
///             }
///
///             // `drain.all()` is also available
///         },
///         RevDirection::FORWARD_LOG => {
///             let iter = log.forward_log(&meta)?;
///             let next_update = iter.update.clone();
///             for next_transition in iter {
///                 let next_transition: MyTransition = next_transition.clone();
///
///                 // mutate some state with the logged transitions and update
///             }
///         },
///         RevDirection::BackwardLog => {
///             let iter = log.backward_log(&meta)?;
///             let previous_update: MyUpdate = iter.update.clone();
///             for previous_transition in iter {
///                 let previous_transition: MyTransition = previous_transition.clone();
///
///                 // mutate some state with the logged transitions and update
///             }
///         }
///     }
///
///     Ok(())
/// }
/// ```
#[derive(Debug)]
pub struct TransitionsLog<T, U = ()> {
    /// Contains the transition values in the order they were pushed.
    transitions: VecDeque<T>,

    /// Contains the update values and the amount of transitions of each update.
    updates: TransitionLog<TransitionsLogUpdate<U>>,

    /// Points to the chronologically next transition in [`Self::transitions`]. If it is equal to
    /// the length of it, the log reached its future end.
    index: usize,
}

impl<T, U> Default for TransitionsLog<T, U> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U> TransitionsLog<T, U> {
    /// Creates an empty log.
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            updates: TransitionLog::new(),
            index: 0,
        }
    }

    /// Creates an empty log with space for at least `transitions_capacity` transitions (`T`) from
    /// at at least `updates_capacity` updates (`U`).
    ///
    /// See [`VecDeque::with_capacity`].
    pub fn with_capacities(transitions_capacity: usize, updates_capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(transitions_capacity),
            updates: TransitionLog::with_capacity(updates_capacity),
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
        self.updates.poison()
    }

    /// Unsets the poison, see [`poison`](Self::poison).
    ///
    /// This happens automatically when
    /// [`pre_update`](Self::pre_update)([`_drain`](Self::pre_update_drain)) applies a previous
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear).
    pub fn clear_poison(&mut self) {
        self.updates.clear_poison();
    }

    /// Returns the number of transitions (`T`) in the log.
    ///
    /// See [`VecDeque::len`].
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }

    /// Returns the number of updates (`U`) in the log.
    ///
    /// See [`VecDeque::len`].
    pub fn updates_len(&self) -> usize {
        self.updates.len()
    }

    /// Returns the number of transitions (`T`) the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`].
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }

    /// Returns the number of updates (`U`) the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`].
    pub fn updates_capacity(&self) -> usize {
        self.updates.capacity()
    }

    /// Returns `true` if the log contains no transitions (`T`).
    ///
    /// See [`VecDeque::is_empty`].
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }

    /// Returns `true` if the log contains no updates (`U`).
    ///
    /// See [`VecDeque::is_empty`].
    pub fn updates_is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    /// Reserves capacity for at least `additional` more transitions (`T`).
    ///
    /// See [`VecDeque::reserve`].
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }

    /// Reserves capacity for at least `additional` more updates (`U`).
    ///
    /// See [`VecDeque::reserve`].
    pub fn updates_reserve(&mut self, additional: usize) {
        self.updates.reserve(additional)
    }

    /// Reserves capacity for at least `additional` more transitions (`T`).
    ///
    /// See [`VecDeque::reserve_exact`].
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }

    /// Reserves capacity for at least `additional` more updates (`U`).
    ///
    /// See [`VecDeque::reserve_exact`].
    pub fn updates_reserve_exact(&mut self, additional: usize) {
        self.updates.reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more transitions (`T`).
    ///
    /// See [`VecDeque::try_reserve`].
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more updates (`U`).
    ///
    /// See [`VecDeque::try_reserve`].
    pub fn updates_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.updates.try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more transitions (`T`).
    ///
    /// See [`VecDeque::try_reserve_exact`].
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more updates (`U`).
    ///
    /// See [`VecDeque::try_reserve_exact`].
    pub fn updates_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.updates.try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log's transitions (`T`) with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log's updates (`U`) with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    pub fn updates_shrink_to(&mut self, min_capacity: usize) {
        self.updates.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log's transitions (`T`) as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    pub fn transitions_shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }

    /// Shrinks the capacity of the log's updates (`U`) as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    pub fn updates_shrink_to_fit(&mut self) {
        self.updates.shrink_to_fit()
    }

    pub fn forward_extend_with<'a, I: IntoIterator<Item = T>>(
        &'a mut self,
        meta: &RevMeta,
        max_past_len: u64,
        transitions: I,
        update: U,
    ) -> Result<TransitionsDrain<'a, T, U, I>, OutOfLog> {
        let updates = self.updates.forward_push(
            meta,
            max_past_len,
            TransitionsLogUpdate {
                update,
                transitions: usize::MAX, // will be overwritten when transititons are counted
            },
        )?;
        let gap_range = if updates.is_clear() {
            GapRange::new_clear(self.index)
        } else {
            let start = updates
                .iter_past()
                .map(|update| update.transitions)
                .sum::<usize>();
            GapRange::new(start, self.index)
        };
        Ok(TransitionsDrain {
            transitions: &mut self.transitions,
            updates,
            index: &mut self.index,
            transitions_iter: Some(transitions),
            gap_range,
            gap_buffer: Default::default(),
        })
    }

    /// Returns references to the log entry that was logged at the chronologically previous push. If
    /// the log is at the past end before this call, this method returns an [`OutOfLog`] error,
    /// leaving the log unchanged.
    ///
    /// The references are returned in an transition (`T`) iterator, see [`TransitionsLogIterMut`].
    /// It contains an accessible reference field to the update (`U`).
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
    /// For examples, see the [type level documentation](TransitionsLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), this always returns an error. If bevy's
    /// `track_location` cargo feature is activated, the error here contains the location where it
    /// originally occured.
    #[track_caller]
    pub fn backward_log(
        &mut self,
        meta: &RevMeta,
    ) -> Result<TransitionsLogIterMut<T, U>, OutOfLog> {
        let old_index = self.index;
        let update_mut = self.updates.backward_log(meta)?;
        self.index -= update_mut.transitions;
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(TransitionsLogIterMut {
            transitions: iter,
            update: &mut update_mut.update,
        })
    }

    /// Returns references to the log entry that was logged at the chronologically next push. If the
    /// log is at the future end before this call, this method returns an [`OutOfLog`] error,
    /// leaving the log unchanged.
    ///
    /// The references are returned in an transition (`T`) iterator, see [`TransitionsLogIterMut`].
    /// It contains an accessible reference field to the update (`U`).
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
    /// For examples, see the [type level documentation](TransitionsLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), this always returns an error. If bevy's
    /// `track_location` cargo feature is activated, the error here contains the location where it
    /// originally occured.
    #[track_caller]
    pub fn forward_log(&mut self, meta: &RevMeta) -> Result<TransitionsLogIterMut<T, U>, OutOfLog> {
        let old_index = self.index;
        let update_mut = self.updates.forward_log(meta)?;
        self.index += update_mut.transitions;
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(TransitionsLogIterMut {
            transitions: iter,
            update: &mut update_mut.update,
        })
    }
}

impl<T> TransitionsLog<T, ()> {
    pub fn forward_extend<'a, I: IntoIterator<Item = T>>(
        &'a mut self,
        meta: &RevMeta,
        max_past_len: u64,
        transitions: I,
    ) -> Result<TransitionsDrain<'a, T, (), I>, OutOfLog> {
        self.forward_extend_with(meta, max_past_len, transitions, ())
    }
}

#[derive(Debug)]
pub struct TransitionsDrain<'a, T, U, I>
where
    I: IntoIterator<Item = T>,
{
    transitions: &'a mut VecDeque<T>,
    updates: TransitionDrain<'a, TransitionsLogUpdate<U>>,
    index: &'a mut usize,
    transitions_iter: Option<I>,
    gap_range: GapRange,
    gap_buffer: Box<[T]>,
}

pub(super) type DrainAndMaybePushedTransitions<'a, T, I> =
    core::iter::Chain<Drain<'a, T>, core::iter::Flatten<core::option::IntoIter<I>>>;

impl<'a, T, U, I> TransitionsDrain<'a, T, U, I>
where
    I: IntoIterator<Item = T>,
{
    /// Returns log entries that were pushed before [`RevMeta::past_end`].
    pub fn past(
        &mut self,
    ) -> TransitionsDrainIters<
        DrainAndMaybePushedTransitions<T, I>,
        DrainAndMaybePushedTransition<TransitionsLogUpdate<U>>,
        U,
    > {
        let end = self.gap_range.drain_past_end();
        let mut maybe_pushed = None;
        if !self.updates.push() {
            maybe_pushed = self.transitions_iter.take();
        }
        let transitions = self
            .transitions
            .drain(..end)
            .chain(maybe_pushed.into_iter().flatten());
        let updates = self.updates.past();
        TransitionsDrainIters {
            transitions,
            updates,
            _m: PhantomData,
        }
    }

    /// Returns log entries that were pushed after [`RevMeta::now`] which, at this point of time,
    /// is equal to [`RevMeta::future_end`].
    pub fn future(&mut self) -> TransitionsDrainIters<Drain<T>, Drain<TransitionsLogUpdate<U>>, U> {
        let start = self.gap_range.drain_future_start();
        let transitions = self.transitions.drain(start..);
        let updates = self.updates.future();
        TransitionsDrainIters {
            transitions,
            updates,
            _m: PhantomData,
        }
    }

    /// Returns log entries that were pushed before [`RevMeta::past_end`] or after [`RevMeta::now`]
    /// which, at this point of time, is equal to [`RevMeta::future_end`].
    pub fn all(
        &mut self,
    ) -> TransitionsDrainIters<DrainAll<T>, DrainAll<TransitionsLogUpdate<U>>, U> {
        let transitions = DrainAll::new(
            &mut self.transitions,
            &mut self.gap_range,
            &mut self.gap_buffer,
        );
        let updates = self.updates.all();
        TransitionsDrainIters {
            transitions,
            updates,
            _m: PhantomData,
        }
    }
}

impl<'a, T, U, I> Drop for TransitionsDrain<'a, T, U, I>
where
    I: IntoIterator<Item = T>,
{
    fn drop(&mut self) {
        if self.gap_range.is_clear() {
            self.transitions.clear();
        } else {
            self.transitions.truncate(self.gap_range.end);
            // todo: use truncate_front https://github.com/rust-lang/rust/issues/140667
            self.transitions.drain(..self.gap_range.start);
        }
        prepend(&mut self.transitions, &mut self.gap_buffer);
        if self.updates.push()
            && let Some(transitions_iter) = self.transitions_iter.take()
        {
            let mut len = self.transitions.len();
            self.transitions.extend(transitions_iter);
            len = self.transitions.len() - len;
            self.updates.transition_mut().unwrap().transitions = len;
        }
        *self.index = self.transitions.len();
    }
}

#[derive(Debug)]
pub struct TransitionsDrainIters<TI, UI, U> {
    pub transitions: TI,
    pub updates: UI,
    _m: PhantomData<U>,
}

impl<TI, UI, U> TransitionsDrainIters<TI, UI, U>
where
    TI: Iterator,
    UI: Iterator<Item = TransitionsLogUpdate<U>>,
{
    /// Returns the transitions and the update of the next log entry from the draining iterators.
    ///
    /// Returns `None` if no more log entries are to drain.
    pub fn next_log_entry(&mut self) -> Option<(Take<&'_ mut TI>, U)> {
        self.updates.next().map(|update| {
            (
                self.transitions.by_ref().take(update.transitions),
                update.update,
            )
        })
    }
}

impl<TI, UI> Iterator for TransitionsDrainIters<TI, UI, ()>
where
    TI: Iterator,
{
    type Item = TI::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.transitions.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.transitions.size_hint()
    }
}

impl<TI, UI> ExactSizeIterator for TransitionsDrainIters<TI, UI, ()>
where
    TI: ExactSizeIterator,
{
    fn len(&self) -> usize {
        self.transitions.len()
    }
}

impl<TI, UI> FusedIterator for TransitionsDrainIters<TI, UI, ()> where TI: FusedIterator {}

/// An iterator over mutable references of the chronologically [next](TransitionsLog::forward_log)
/// or [previous](TransitionsLog::backward_log) transitions (`T`).
///
/// A mutable reference to the update (`U`) of the log entry is accessible as the
/// [`update` field](Self::update).
#[derive(Debug)]
pub struct TransitionsLogIterMut<'a, T, U> {
    /// Mutable references to the transitions of this log entry.
    transitions: IterMut<'a, T>,

    /// A mutable reference to the update of this log entry.
    pub update: &'a mut U,
}

impl<'a, T, U> Iterator for TransitionsLogIterMut<'a, T, U> {
    type Item = &'a mut T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.transitions.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.transitions.size_hint()
    }

    fn fold<Acc, F>(self, accum: Acc, f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        self.transitions.fold(accum, f)
    }

    #[inline]
    fn last(self) -> Option<&'a mut T> {
        self.transitions.last()
    }
}

impl<'a, T, U> DoubleEndedIterator for TransitionsLogIterMut<'a, T, U> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a mut T> {
        self.transitions.next_back()
    }

    fn rfold<Acc, F>(self, accum: Acc, f: F) -> Acc
    where
        F: FnMut(Acc, Self::Item) -> Acc,
    {
        self.transitions.rfold(accum, f)
    }
}

impl<T, U> ExactSizeIterator for TransitionsLogIterMut<'_, T, U> {
    fn len(&self) -> usize {
        self.transitions.len()
    }
}

impl<T, U> FusedIterator for TransitionsLogIterMut<'_, T, U> {}

/// An internal wrapper around the update and the amount of transitions that belong to
/// a log entry.
#[derive(Debug, Clone)]
pub struct TransitionsLogUpdate<U> {
    /// The update value of this log entry.
    pub update: U,

    /// The amount of transitions that belong to this log entry
    ///
    /// Must be private because draining iterators rely on the value to remain unchanged.
    transitions: usize,
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
        logs: Logs<TransitionsLog<char, char>>,
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
            past_drain: [(&'static str, char); N],
            future_drain: [(&'static str, char); M],
            push: (&'static str, char),
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
                self.logs.assert_forward_transitions(
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
        fn forward_log(&mut self, expected: Result<(&'static str, char), ()>) {
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            match expected {
                Ok(_) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        self.logs.assert_forward_log_transitions(meta, expected);
                    });
                }
                Err(_) => {
                    self.meta.update_ref(Ok(false), |_, _| ());
                    self.logs
                        .assert_forward_log_transitions(&self.meta, expected);
                }
            }
        }
        #[track_caller]
        fn backward_log(&mut self, expected: Result<(&'static str, char), ()>) {
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            match expected {
                Ok(_) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        self.logs.assert_backward_log_transitions(meta, expected);
                    });
                }
                Err(_) => {
                    self.meta.update_ref(Ok(false), |_, _| ());
                    self.logs
                        .assert_backward_log_transitions(&self.meta, expected);
                }
            }
        }
    }

    #[test]
    fn traverses_log() {
        use crate::log::test::transitions_presets::*;

        let mut meta_and_logs = MetaAndLogs::new(5);

        meta_and_logs.forward([], [], A, false);
        meta_and_logs.forward([], [], B, false);
        meta_and_logs.forward([], [], C, false);
        meta_and_logs.forward([], [], D, false);
        meta_and_logs.forward([A], [], E, false); // non-past draining remove a() here
        meta_and_logs.forward([B], [], F, false);

        meta_and_logs.backward_log(Ok(F));
        meta_and_logs.backward_log(Ok(E));
        meta_and_logs.backward_log(Ok(D));
        meta_and_logs.backward_log(Ok(C));
        meta_and_logs.backward_log(Err(())); // b() is unreachable but not yet drained

        meta_and_logs.forward_log(Ok(C));
        meta_and_logs.forward_log(Ok(D));
        meta_and_logs.forward_log(Ok(E));
        meta_and_logs.forward_log(Ok(F));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log(Ok(F));
        meta_and_logs.backward_log(Ok(E));

        meta_and_logs.forward([], [E, F], G, false);

        meta_and_logs.backward_log(Ok(G));
        meta_and_logs.backward_log(Ok(D));
        meta_and_logs.backward_log(Ok(C));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward_log(Ok(C));
        meta_and_logs.forward_log(Ok(D));
        meta_and_logs.forward_log(Ok(G));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log(Ok(G));
        meta_and_logs.backward_log(Ok(D));

        meta_and_logs.forward([C], [D, G], H, true);

        meta_and_logs.backward_log(Ok(H));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward_log(Ok(H));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.forward([], [], I, false);

        meta_and_logs.backward_log(Ok(I));

        meta_and_logs.noop_forward_backward_log();

        meta_and_logs.backward_log(Ok(H));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward([], [H, I], J, false);
        meta_and_logs.forward([], [], K, false);
        meta_and_logs.forward([], [], L, false);

        meta_and_logs.backward_log(Ok(L));

        meta_and_logs
            .meta
            .set_max_world_states(NonZeroU64::new(2).unwrap());
        meta_and_logs.forward([J, K], [L], M, false);
    }
}
