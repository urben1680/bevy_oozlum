use crate::{
    log::{
        prepend, DrainAll, GapRange, OutOfLog, TransitionDrain, TransitionLog
    },
    meta::RevMeta,
};
use core::{
    fmt::Debug,
    iter::{FusedIterator, Take},
    marker::PhantomData,
};
use std::{
    collections::{
        TryReserveError, VecDeque,
        vec_deque::{Drain, IterMut},
    },
    mem::ManuallyDrop,
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
///     log.pre_update(&meta);
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///             let more_new_transitions: Vec<MyTransition> = todo!();
///             let new_update: MyUpdate = todo!();
///
///             // mutate some state with the new transitions and update
///
///             // push transitions and update to the log
///             log.push(meta.past_len(), |mut log| {
///                 log.push(new_transition);
///                 log.extend(more_new_transitions.into_iter());
///                 new_update
///             });
///         },
///         RevDirection::FORWARD_LOG => {
///             let iter = log.forward_log()?;
///             let next_update = iter.update.clone();
///             for next_transition in iter {
///                 let next_transition: MyTransition = next_transition.clone();
///
///                 // mutate some state with the logged transitions and update
///             }
///         },
///         RevDirection::BackwardLog => {
///             let iter = log.backward_log()?;
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
///
/// ## Draining future
///
/// There may be cases where extra cleanup is needed in which case the transitions that the log no
/// longer needs should be drained with in an iterator.
///
/// The type returned by `pre_update_drain` is more complex than the
/// [`TransitionLog` variant](TransitionLog::pre_update_drain) as it is not just a single iterator
/// but two, one over the transitions (`T`) and one over the updates (`U`). The iterator fields are
/// freely accessible, but if the draining needs to be done in update chunks, use the
/// [`next_log_entry`](TransitionsDrainChunkable::next_log_entry) method that returns an
/// `Option<(impl Iterator<Item = T>, U)>`. In every case, the iterators yield the items in the
/// order they were pushed.
///
/// In this example the cleanup is required for log entries that are in the future part.
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
///     let drains = log.pre_update_drain(&meta);
///
///     let mut future_drains = drains.future();
///     // if no update chunks are needed, use `transitions` and `updates` fields of `future_drains`
///     while let Some((iter, future_update)) = future_drains.next_log_entry() {
///         for future_transition in iter {
///
///             // do cleanup tasks with future transitions and updates
///         }
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///             let more_new_transitions: Vec<MyTransition> = todo!();
///             let new_update: MyUpdate = todo!();
///
///             // mutate some state with the new transitions and update
///
///             // push transitions and update to the log
///             log.push(meta.past_len(), |mut log| {
///                 log.push(new_transition);
///                 log.extend(more_new_transitions.into_iter());
///                 new_update
///             });
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
/// # #[derive(Clone)]
/// # struct MyUpdate;
/// fn system(
///     meta: Res<RevMeta>,
///     mut log: Local<TransitionsLog<MyTransition, MyUpdate>>
/// ) -> Result<(), BevyError> {
///     let mut drains = log.pre_update_drain(&meta);
///
///     let mut past_drains = drains.past();
///     // if no update chunks are needed, use `transitions` and `updates` fields of `past_drains`
///     while let Some((iter, past_update)) = past_drains.next_log_entry() {
///         for past_transition in iter {
///
///             // do cleanup tasks with past transitions and updates
///         }
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///             let more_new_transitions: Vec<MyTransition> = todo!();
///             let new_update: MyUpdate = todo!();
///
///             // mutate some state with the new transitions and update
///
///             // push transitions and update to the log
///             let mut drain_past = log.push_drain_past(meta.past_len(), |mut log| {
///                 log.push(new_transition);
///                 log.extend(more_new_transitions.into_iter());
///                 new_update
///             });
///
///             while let Some((iter, past_update)) = drain_past.next_log_entry() {
///                 for past_transition in iter {
///
///                     // do cleanup tasks with past transitions and updates
///                 }
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
/// The `MyTransition` and/or `MyUpdate` may for example contain an
/// [entity ID](bevy_ecs::entity::Entity). This could be the ID of a temporal entity that is
/// associated to this transition. The cleanup then would be to despawn it.
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
///     let mut drains = log.pre_update_drain(&meta);
///
///     let mut past_drains = drains.past();
///     while let Some((iter, past_update)) = past_drains.next_log_entry() {
///         for past_transition in iter {
///
///             // do cleanup tasks with past transitions and updates
///         }
///     }
///
///     let mut future_drains = drains.future();
///     while let Some((iter, future_update)) = future_drains.next_log_entry() {
///         for future_transition in iter {
///
///             // do cleanup tasks with future transitions and updates
///         }
///     }
///
///     // or, instead of the two above: `let mut all_drains = drains.all()`
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition: MyTransition = todo!();
///             let more_new_transitions: Vec<MyTransition> = todo!();
///             let new_update: MyUpdate = todo!();
///
///             // mutate some state with the new transitions and update
///
///             // push transitions and update to the log
///             let mut drain_past = log.push_drain_past(meta.past_len(), |mut log| {
///                 log.push(new_transition);
///                 log.extend(more_new_transitions.into_iter());
///                 new_update
///             });
///
///             while let Some((iter, past_update)) = drain_past.next_log_entry() {
///                 for past_transition in iter {
///
///                     // do cleanup tasks with past transitions and updates
///                 }
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

    pub fn extend_with<'a, I: IntoIterator<Item = T>>(
        &'a mut self,
        meta: &RevMeta,
        max_past_len: u64,
        transitions: I,
        update: U,
    ) -> Result<TransitionsDrain<'a, T, U, I>, OutOfLog> {
        let updates = self.updates.push(
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
            let mut start_offset = 0;
            let start = updates
                .iter_past()
                .map(|update| {
                    start_offset = update.transitions;
                    start_offset
                })
                .sum::<usize>();
            GapRange {
                start,
                start_offset,
                end: self.index,
            }
        };
        Ok(TransitionsDrain {
            transitions: &mut self.transitions,
            updates,
            index: &mut self.index,
            transitions_iter: ManuallyDrop::new(transitions),
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
    pub fn extend<'a, I: IntoIterator<Item = T>>(
        &'a mut self,
        meta: &RevMeta,
        max_past_len: u64,
        transitions: I,
    ) -> Result<TransitionsDrain<'a, T, (), I>, OutOfLog> {
        self.extend_with(meta, max_past_len, transitions, ())
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
    transitions_iter: ManuallyDrop<I>,
    gap_range: GapRange,
    gap_buffer: Box<[T]>,
}

impl<'a, T, U, I> TransitionsDrain<'a, T, U, I>
where
    I: IntoIterator<Item = T>,
{
    pub fn drain_past(
        &mut self,
    ) -> TransitionsDrainIters<Drain<T>, Drain<TransitionsLogUpdate<U>>, U> {
        let end = self.gap_range.drain_past_end();
        let transitions = self.transitions.drain(..end);
        let updates = self.updates.drain_past();
        println!("drain_past end: {end}, transitions.len(): {}, updates.len(): {}", transitions.len(), updates.len());
        TransitionsDrainIters {
            transitions,
            updates,
            _m: PhantomData,
        }
    }

    pub fn drain_future(
        &mut self,
    ) -> TransitionsDrainIters<Drain<T>, Drain<TransitionsLogUpdate<U>>, U> {
        let start = self.gap_range.drain_future_start();
        let transitions = self.transitions.drain(start..);
        let updates = self.updates.drain_future();
        TransitionsDrainIters {
            transitions,
            updates,
            _m: PhantomData,
        }
    }

    pub fn drain_all(
        &mut self,
    ) -> TransitionsDrainIters<DrainAll<T>, DrainAll<TransitionsLogUpdate<U>>, U> {
        let transitions = DrainAll::new(
            &mut self.transitions,
            &mut self.gap_range,
            &mut self.gap_buffer,
        );
        let updates = self.updates.drain_all();
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
        if self.gap_range.is_clear() || self.gap_range.start == self.gap_range.end {
            self.transitions.clear();
        } else {
            self.transitions.truncate(self.gap_range.end);
            // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
            self.transitions.drain(..self.gap_range.start);
        }
        prepend(&mut self.gap_buffer, &mut self.transitions);
        let transitions_iter = unsafe {
            // SAFETY: only called this once in Drop
            ManuallyDrop::take(&mut self.transitions_iter)
        };
        if self.updates.does_push() {
            let mut len = self.transitions.len();
            self.transitions.extend(transitions_iter);
            len = self.transitions.len() - len;
            self.updates.transition_mut().transitions = len;
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
    TI: ExactSizeIterator,
    UI: ExactSizeIterator<Item = TransitionsLogUpdate<U>>,
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
    use core::{num::NonZeroU64, str::Chars};

    use crate::meta::{RevDirection, RevQueue};

    use super::*;

    #[derive(Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        drop_drain: TransitionsLog<char, char>,
        past_drain: TransitionsLog<char, char>,
        future_drain: TransitionsLog<char, char>,
        past_future_drain: TransitionsLog<char, char>,
        future_past_drain: TransitionsLog<char, char>,
        all_drain: TransitionsLog<char, char>,
        past_all_drain: TransitionsLog<char, char>,
        future_all_drain: TransitionsLog<char, char>,
    }

    impl<TI, UI> TransitionsDrainIters<TI, UI, char>
    where
        TI: ExactSizeIterator<Item = char>,
        UI: ExactSizeIterator<Item = TransitionsLogUpdate<char>>,
    {
        fn to_tuples(mut self) -> Vec<(String, char)> {
            let mut v = Vec::new();
            while let Some((transitions, update)) = self.next_log_entry() {
                v.push((transitions.collect(), update))
            }
            v
        }
    }

    impl<'a> TransitionsLogIterMut<'a, char, char> {
        fn to_tuple(self) -> (String, char) {
            let update = *self.update;
            (self.map(|char| *char).collect(), update)
        }
    }

    impl TransitionsDrain<'_, char, char, Chars<'_>> {
        fn assert_past<const N: usize>(&mut self, expected: [(String, char); N]) -> &mut Self {
            println!("{self:#?}");
            let iter = self.drain_past();
            let len = expected
                .iter()
                .map(|(s, _)| s.chars().count())
                .sum::<usize>();
            assert_eq!(iter.transitions.len(), len);
            assert_eq!(iter.updates.len(), N);
            let actual = iter.to_tuples();
            assert_eq!(actual, expected);
            if N != 0 {
                let iter = self.drain_past();
                assert_eq!(iter.transitions.len(), 0);
                assert_eq!(iter.transitions.count(), 0);
                assert_eq!(iter.updates.len(), 0);
                assert_eq!(iter.updates.count(), 0);
            }
            self
        }
        fn assert_future<const N: usize>(&mut self, expected: [(String, char); N]) -> &mut Self {
            let iter = self.drain_future();
            let len = expected
                .iter()
                .map(|(s, _)| s.chars().count())
                .sum::<usize>();
            assert_eq!(iter.transitions.len(), len);
            assert_eq!(iter.updates.len(), N);
            let actual = iter.to_tuples();
            assert_eq!(actual, expected);
            if N != 0 {
                let iter = self.drain_future();
                assert_eq!(iter.transitions.len(), 0);
                assert_eq!(iter.transitions.count(), 0);
                assert_eq!(iter.updates.len(), 0);
                assert_eq!(iter.updates.count(), 0);
            }
            self
        }
        fn assert_all<const N: usize, const M: usize>(
            &mut self,
            past: [(String, char); N],
            future: [(String, char); M],
        ) -> &mut Self {
            let iter = self.drain_all();
            let len = past
                .iter()
                .chain(future.iter())
                .map(|(s, _)| s.chars().count())
                .sum::<usize>();
            assert_eq!(iter.transitions.len(), len);
            assert_eq!(iter.updates.len(), N + M);
            let actual = iter.to_tuples();
            let expected = past.into_iter().chain(future).collect::<Vec<_>>();
            assert_eq!(actual, expected);
            if N + M != 0 {
                let iter = self.drain_all();
                assert_eq!(iter.transitions.len(), 0);
                assert_eq!(iter.transitions.count(), 0);
                assert_eq!(iter.updates.len(), 0);
                assert_eq!(iter.updates.count(), 0);
            }
            self
        }
    }

    impl MetaAndLogs {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(NonZeroU64::new(max_world_states), false),
                drop_drain: TransitionsLog::new(),
                past_drain: TransitionsLog::new(),
                future_drain: TransitionsLog::new(),
                past_future_drain: TransitionsLog::new(),
                future_past_drain: TransitionsLog::new(),
                all_drain: TransitionsLog::new(),
                past_all_drain: TransitionsLog::new(),
                future_all_drain: TransitionsLog::new(),
            }
        }
        fn forward<const N: usize, const M: usize>(
            &mut self,
            past_drain: [(String, char); N],
            future_drain: [(String, char); M],
            (transitions, update): (String, char),
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

                self.drop_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap();

                println!();
                self.past_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap()
                    .assert_past(past_drain.clone());
                println!("{past_drain:?} -> {:#?}", self.past_drain);
                assert_ne!(update, 'B');

                self.future_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap()
                    .assert_future(future_drain.clone());

                self.past_future_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap()
                    .assert_past(past_drain.clone())
                    .assert_future(future_drain.clone())
                    .assert_all([], []);

                self.future_past_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap()
                    .assert_future(future_drain.clone())
                    .assert_past(past_drain.clone())
                    .assert_all([], []);

                self.all_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap()
                    .assert_all(past_drain.clone(), future_drain.clone());

                self.past_all_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap()
                    .assert_past(past_drain.clone())
                    .assert_all([], future_drain.clone())
                    .assert_future([]);

                self.future_all_drain
                    .extend_with(meta, meta.past_len(), transitions.chars(), update)
                    .unwrap()
                    .assert_future(future_drain)
                    .assert_all(past_drain, [])
                    .assert_future([]);
            });
        }
        fn noop_forward_backward_log(&mut self) {
            self.meta.set_queue(RevQueue::RUN_NOT_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
        }
        #[track_caller]
        fn forward_log(&mut self, expected: Result<(String, char), ()>) {
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
                            let actual = log.forward_log(meta).map(TransitionsLogIterMut::to_tuple);
                            assert_eq!(actual, Ok(expected.clone()));
                        }
                    });
                }
                Err(()) => {
                    self.meta.update_ref(Ok(false), |_, _| ());

                    for log in logs {
                        assert_eq!(
                            log.forward_log(&self.meta)
                                .map(TransitionsLogIterMut::to_tuple),
                            Err(OutOfLog::caller())
                        );
                        log.clear_poison();
                    }
                }
            }
        }
        #[track_caller]
        fn backward_log(&mut self, expected: Result<(String, char), ()>) {
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
                            let actual =
                                log.backward_log(meta).map(TransitionsLogIterMut::to_tuple);
                            assert_eq!(actual, Ok(expected.clone()));
                        }
                    });
                }
                Err(()) => {
                    self.meta.update_ref(Ok(false), |_, _| ());

                    for log in [&mut self.drop_drain, &mut self.future_drain] {
                        assert_eq!(
                            log.backward_log(&self.meta)
                                .map(TransitionsLogIterMut::to_tuple),
                            Err(OutOfLog::caller())
                        );
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
                                let expected = expected.to_tuple();
                                assert_eq!(
                                    log.backward_log(&self.meta)
                                        .map(TransitionsLogIterMut::to_tuple),
                                    Err(OutOfLog::caller()),
                                    "{i}"
                                );
                                log.clear_poison();

                                // undo Ok
                                let actual = log
                                    .forward_log(&self.meta)
                                    .map(TransitionsLogIterMut::to_tuple);
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

        let a = || ("a".repeat(1), 'A');
        let b = || ("b".repeat(2), 'B');
        let c = || ("c".repeat(3), 'C');
        let d = || ("d".repeat(4), 'D');
        let e = || ("e".repeat(5), 'E');
        let f = || ("f".repeat(6), 'F');
        let g = || ("g".repeat(7), 'G');
        let h = || ("h".repeat(8), 'H');
        let i = || ("i".repeat(9), 'I');
        let j = || ("j".repeat(10), 'J');
        let k = || ("k".repeat(11), 'K');
        let l = || ("l".repeat(12), 'L');
        let m = || ("m".repeat(13), 'M');

        meta_and_logs.forward([], [], a(), false);
        meta_and_logs.forward([], [], b(), false);
        meta_and_logs.forward([], [], c(), false);
        meta_and_logs.forward([], [], d(), false);
        meta_and_logs.forward([], [], e(), false); // non-past draining remove a() here
        meta_and_logs.forward([a()], [], f(), false);

        meta_and_logs.backward_log(Ok(f()));
        meta_and_logs.backward_log(Ok(e()));
        meta_and_logs.backward_log(Ok(d()));
        meta_and_logs.backward_log(Ok(c()));
        meta_and_logs.backward_log(Err(())); // b() is unreachable but not yet drained

        meta_and_logs.forward_log(Ok(c()));
        meta_and_logs.forward_log(Ok(d()));
        meta_and_logs.forward_log(Ok(e()));
        meta_and_logs.forward_log(Ok(f()));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log(Ok(f()));
        meta_and_logs.backward_log(Ok(e()));

        meta_and_logs.forward([], [e(), f()], g(), false);

        meta_and_logs.backward_log(Ok(g()));
        meta_and_logs.backward_log(Ok(d()));
        meta_and_logs.backward_log(Ok(c()));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward_log(Ok(c()));
        meta_and_logs.forward_log(Ok(d()));
        meta_and_logs.forward_log(Ok(g()));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log(Ok(g()));
        meta_and_logs.backward_log(Ok(d()));

        meta_and_logs.forward([b(), c()], [d(), g()], h(), true);

        meta_and_logs.backward_log(Ok(h()));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward_log(Ok(h()));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.forward([], [], i(), false);

        meta_and_logs.backward_log(Ok(i()));

        meta_and_logs.noop_forward_backward_log();

        meta_and_logs.backward_log(Ok(h()));
        meta_and_logs.backward_log(Err(()));

        meta_and_logs.forward([], [h(), i()], j(), false);
        meta_and_logs.forward([], [], k(), false);
        meta_and_logs.forward([], [], l(), false);

        meta_and_logs.backward_log(Ok(l()));

        meta_and_logs
            .meta
            .set_max_world_states(NonZeroU64::new(2).unwrap());
        meta_and_logs.forward([j()], [l()], m(), false);
    }
}
