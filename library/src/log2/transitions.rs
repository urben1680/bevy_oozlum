use crate::{
    log2::{
        OutOfLog, PreUpdateKind, TransitionDrainAll, TransitionDrainFuture, TransitionDrainPast,
        TransitionLog, transition::TransitionDrains,
    },
    meta::RevMeta,
};
use bevy_log::error;
use core::{
    any::type_name,
    fmt::Debug,
    iter::{FusedIterator, Take},
    marker::PhantomData,
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

    /// Pushes transitions (`T`) and an update (`U`) to the log and then shortens the log to contain
    /// past log entries.
    ///
    /// The closure offers different ways to push the transitions, see [`LogMut`], but also allows
    /// to not push any at all. This is still considered as an update however.
    ///
    /// To receive an iterator over the removed log entries that got out-of-log, use
    /// [`push_drain_past`](Self::push_drain_past) instead. Note that the `max_past_len`
    /// logic works a bit differently with that method, see its documentation.
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) or
    /// [`pre_update_drain`](Self::pre_update_drain) **must** be called at least once in the
    /// [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// For examples, see the [type level documentation](TransitionsLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), the pushed log entry will not be logged, no log
    /// entries will be truncated and an [error will be logged](bevy_log::error).
    #[track_caller]
    pub fn push(&mut self, max_past_len: u64, c: impl FnOnce(LogMut<T, U>) -> U) {
        if self.poison().is_err() {
            return Self::poison_push_err();
        }
        let (transition_drain, amount_drain) =
            self.push_and_get_transition_and_update_drain(max_past_len, c);
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..transition_drain);
        self.updates.drain_past(amount_drain);
    }

    /// Pushes transitions (`T`) and an update (`U`) to the log and then shortens the log to contain
    /// at most `max_past_len + 1` past updates.
    ///
    /// The closure offers different ways to push the transitions, see [`LogMut`], but also allows
    /// to not push any at all. This is still considered as an update however.
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    ///
    /// Before calling this, [`pre_update_drain`](Self::pre_update_drain) **must** be called at
    /// least once in the [present reversible frame](RevMeta::now). This method may panic if this
    /// was not done. Do not use [`pre_update`](Self::pre_update) because that method is needed as
    /// well to receive past log entries that are now out-of-log.
    ///
    /// This methods returns log entries that got out-of-log because the push of `transition` and a
    /// potential reduction of `max_past_len` form the last update exceed the current
    /// `max_past_len` value. If there are no such log entries, the iterator is empty.
    ///
    /// The `max_past_len` addition by one is done to ensure that drained log entries were pushed at
    /// a frame that is now out-of-log. This may not be needed in some cases, but is important in
    /// others like when log entries contain an entity [ID](bevy_ecs::entity::Entity) that needs to
    /// remain alive as long it was spawned within the global log range. This makes this method less
    /// likely a source of bugs in the user logic.
    ///
    /// As an explaination why this log type would drain log entries earlier: Only the log entry to
    /// transition _from the very first state_ that can be reverted to _to the second state_ is
    /// required to be stored to cover a past length of `max_past_len`. The log entry to transition
    /// _to the first state_ is otherwise not useful because it would only be used to transition to
    /// and from the state before that which is now out-of-log.
    ///
    /// So if this addition by one was not done, the log entry pushed at [`RevMeta::past_end`], a
    /// frame that is still in-log, would be drained here.
    ///
    /// It is assumed that storing one additionally log entry is not an issue for the user and is
    /// preferred over a potential source of bugs.
    ///
    /// For examples, see the [type level documentation](TransitionsLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), the pushed log entry will not be logged, no log
    /// entries will be drained and an [error will be logged](bevy_log::error).
    #[track_caller]
    pub fn push_drain_past(
        &mut self,
        max_past_len: u64,
        c: impl FnOnce(LogMut<T, U>) -> U,
    ) -> TransitionsDrainAll<T, U> {
        let (transition_drain, amount_drain) = match self.poison() {
            Ok(()) => self.push_and_get_transition_and_update_drain(max_past_len + 1, c),
            Err(_) => {
                Self::poison_push_err();
                (0, 0)
            }
        };
        TransitionsDrainChunkable {
            transitions: self.transitions.drain(..transition_drain),
            updates: self.updates.drain_past(amount_drain),
            _p: PhantomData,
        }
    }

    #[track_caller]
    fn poison_push_err() {
        error!(
            "did not push `{}` to `TransitionsLog` because log is poisoned",
            type_name::<T>()
        );
    }

    /// Applies the pushing closure and returns how many transitions (first tuple element) and how
    /// many updates (second tuple element) need to be removed from the past end of the log to
    /// contain at most `max_past_len` elements.
    fn push_and_get_transition_and_update_drain(
        &mut self,
        max_past_len: u64,
        c: impl FnOnce(LogMut<T, U>) -> U,
    ) -> (usize, usize) {
        assert_eq!(self.index, self.transitions.len()); // do not truncate here, call pre_update!
        let log_mut = LogMut {
            transitions: &mut self.transitions,
            updates: &mut self.updates,
        };
        let update = c(log_mut);
        let pushed_amount = self.transitions.len() - self.index;
        let update = TransitionsLogUpdate {
            update,
            transitions: pushed_amount,
        };
        self.index = self.transitions.len();
        let to_drain = self
            .updates
            .push_and_iter_to_drain_past(max_past_len, update);
        let update_drain = to_drain.len();
        let transition_drain: usize = to_drain.map(|update| update.transitions).sum();
        self.index -= transition_drain;
        (transition_drain, update_drain)
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
    pub fn backward_log(&mut self) -> Result<TransitionsLogIterMut<T, U>, OutOfLog> {
        let old_index = self.index;
        let update_mut = self.updates.backward_log()?;
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
    pub fn forward_log(&mut self) -> Result<TransitionsLogIterMut<T, U>, OutOfLog> {
        let old_index = self.index;
        let update_mut = self.updates.forward_log()?;
        self.index += update_mut.transitions;
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(TransitionsLogIterMut {
            transitions: iter,
            update: &mut update_mut.update,
        })
    }

    /// This method **must** be called once per [reversible frame](RevMeta::now) before any other
    /// mutation.
    ///
    /// This may remove log entries. If a draining iterator for these entries is required, use
    /// [`pre_update_drain`](Self::pre_update_drain) instead.
    ///
    /// For examples, see the [type level documentation](TransitionsLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), this never truncates any log entries, except if
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) is applied, which also clears the poison.
    pub fn pre_update(&mut self, meta: &RevMeta) {
        match self.updates.pre_update_kind(meta) {
            PreUpdateKind::RemoveLog => {
                self.transitions.clear();
                self.updates.clear();
                self.index = 0;
            }
            PreUpdateKind::RemoveFuture => {
                self.transitions.truncate(self.index);
                self.updates.truncate_future();
            }
            PreUpdateKind::Nothing => {}
        }
    }

    /// This method **must** be called once per [reversible frame](RevMeta::now) before any other
    /// mutation.
    ///
    /// This may remove log entries which can be received with the returned iterators. The iterators
    /// may be empty. If no iterators are required, use [`pre_update`](Self::pre_update) instead.
    ///
    /// For examples, see the [type level documentation](TransitionsLog).
    ///
    /// # Poisoning
    ///
    /// If this log [is poisoned](Self::poison), this never drains any log entries, except if
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) is applied, which also clears the poison.
    pub fn pre_update_drain<'a>(&'a mut self, meta: &RevMeta) -> TransitionsDrains<'a, T, U> {
        match self.updates.pre_update_kind(meta) {
            PreUpdateKind::RemoveLog => {
                let past_len = self.index;
                self.index = 0;
                TransitionsDrains {
                    transitions: self.transitions.drain(..),
                    updates: self.updates.full_drain(),
                    past_len,
                }
            }
            PreUpdateKind::RemoveFuture => TransitionsDrains {
                transitions: self.transitions.drain(self.index..),
                updates: self.updates.drain_future(),
                past_len: 0,
            },
            PreUpdateKind::Nothing => TransitionsDrains {
                transitions: self.transitions.drain(..0),
                updates: self.updates.empty_drain(),
                past_len: 0,
            },
        }
    }
}

/// A [`&mut VecDeque<T>`](VecDeque) wrapper that only exposes methods which add to the deque.
///
/// Also offers length and capacity methods for transitions (`T`) and updates (`U`) that do not
/// affect the stored log entries.
pub struct LogMut<'a, T, U> {
    transitions: &'a mut VecDeque<T>,
    updates: &'a mut TransitionLog<TransitionsLogUpdate<U>>,
}

impl<'a, T, U> LogMut<'a, T, U> {
    /// Moves all `transitions` into the log, leaving `transitions` empty.
    ///
    /// See [`VecDeque::append`].
    pub fn append(&mut self, transitions: &mut VecDeque<T>) {
        self.transitions.append(transitions);
    }

    /// Pushes the `transition` into the log.
    ///
    /// See [`VecDeque::push_back`].
    pub fn push(&mut self, transition: T) {
        self.transitions.push_back(transition);
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
}

impl<'a, T, U> Extend<T> for LogMut<'a, T, U> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.transitions.extend(iter);
    }
}

impl<'a, T, U> Extend<&'a T> for LogMut<'a, T, U>
where
    T: 'a + Copy,
{
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.transitions.extend(iter);
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

/// Contains iterators for the past and future log entries that need to be removed at this frame.
///
/// These iterators may be empty if there is nothing out-of-log to drain.
///
/// Returned by [`TransitionsLog::pre_update_drain`].
pub struct TransitionsDrains<'log, T, U> {
    /// Draining iterator containing transitions of either the future or the whole log, or is empty.
    transitions: Drain<'log, T>,

    /// Draining iterator containing updates of either the future or the whole log, or is empty.
    updates: TransitionDrains<'log, TransitionsLogUpdate<U>>,

    /// Amount of transitions in [`Self::transitions`] that are in the past. [`Self::updates`]
    /// tracks its past length itself.
    past_len: usize,
}

impl<'log, T, U> TransitionsDrains<'log, T, U> {
    /// Drains all transitions (`T`) and updates (`U`) that are so far in the past that they are
    /// out-of-log.
    ///
    /// The values are returned in chronological order.
    ///
    /// Calling this method a second time will return empty iterators.
    ///
    /// See [`VecDeque::drain`].
    pub fn past<'a>(&'a mut self) -> TransitionsDrainPast<'a, 'log, T, U> {
        TransitionsDrainChunkable {
            transitions: TransitionDrainPast::new(&mut self.transitions, &mut self.past_len),
            updates: self.updates.past(),
            _p: PhantomData,
        }
    }

    /// Drains all transitions (`T`) and updates (`U`) that are in the future and thus became
    /// out-of-log.
    ///
    /// The values are returned in chronological order.
    ///
    /// This consumes `self`. If the past values are of interest, use [`past`](Self::past) first.
    ///
    /// See [`VecDeque::drain`].
    pub fn future(self) -> TransitionsDrainFuture<'log, T, U> {
        TransitionsDrainChunkable {
            transitions: self.transitions.skip(self.past_len),
            updates: self.updates.future(),
            _p: PhantomData,
        }
    }

    /// Drains all transitions (`T`) and updates (`U`) that are so far in the past that they are
    /// out-of-log, or are in the future and thus became out-of-log.
    ///
    /// The values are returned in chronological order.
    ///
    /// If the past and future values need to be separated or one or the other are not of interest,
    /// use [`past`](Self::past) and/or [`future`](Self::future).
    ///
    /// See [`VecDeque::drain`].
    pub fn all(self) -> TransitionsDrainAll<'log, T, U> {
        TransitionsDrainChunkable {
            transitions: self.transitions,
            updates: self.updates.all(),
            _p: PhantomData,
        }
    }
}

/// Draining transition iterator returned by [`TransitionsDrains::past`].
pub type TransitionsDrainPast<'a, 'log, T, U> = TransitionsDrainChunkable<
    TransitionDrainPast<'a, 'log, T>,
    TransitionDrainPast<'a, 'log, TransitionsLogUpdate<U>>,
    U,
>;

/// Draining transition iterator returned by [`TransitionsDrains::future`].
pub type TransitionsDrainFuture<'log, T, U> = TransitionsDrainChunkable<
    TransitionDrainFuture<'log, T>,
    TransitionDrainFuture<'log, TransitionsLogUpdate<U>>,
    U,
>;

/// Draining transition iterator returned by [`TransitionsDrains::all`].
///
/// Is also returned by [`TransitionsLog::push_drain_past`] but in that case contains only
/// past log entries.
pub type TransitionsDrainAll<'log, T, U> = TransitionsDrainChunkable<
    TransitionDrainAll<'log, T>,
    TransitionDrainAll<'log, TransitionsLogUpdate<U>>,
    U,
>;

/// A wrapper of iterators returned by [`TransitionsDrains`] methods.
///
/// The fields [`transitions`](Self::transitions) and [`updates`](Self::updates) allow direct
/// access to the values of all drained log entries.
///
/// To drain transitions and updates in steps of log entries, the
/// [`next_log_entry`](Self::next_log_entry) method in a `while` loop.
pub struct TransitionsDrainChunkable<TI, UI, U>
where
    TI: ExactSizeIterator,
    UI: ExactSizeIterator<Item = TransitionsLogUpdate<U>>,
{
    /// Draining iterator of transitions from potentially multiple log entries.
    pub transitions: TI,

    /// Draining iterator of updates from potentially multiple log entries.
    pub updates: UI,

    _p: PhantomData<U>,
}

impl<TI, UI, U> TransitionsDrainChunkable<TI, UI, U>
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

#[cfg(test)]
mod test {
    use core::num::NonZeroU64;

    use crate::meta::{RevDirection, RevQueue};

    use super::*;

    struct MetaAndLogs {
        meta: RevMeta,
        with_past_drain: TransitionsLog<char, char>,
        without_past_drain: TransitionsLog<char, char>,
    }

    impl<TI, UI> TransitionsDrainChunkable<TI, UI, char>
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

    impl MetaAndLogs {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(NonZeroU64::new(max_world_states), false),
                with_past_drain: TransitionsLog::new(),
                without_past_drain: TransitionsLog::new(),
            }
        }
        fn forward<const N: usize, const M: usize>(
            &mut self,
            past_drain: [(String, char); N],
            future_drain: [(String, char); M],
            push: (String, char),
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

                // with_past_drain
                let mut drain = self.with_past_drain.pre_update_drain(meta);
                if clear {
                    assert_eq!(drain.past().to_tuples(), past_drain);
                } else {
                    assert_eq!(drain.past().to_tuples(), []);
                }
                assert_eq!(drain.future().to_tuples(), future_drain);
                let drain = self
                    .with_past_drain
                    .push_drain_past(meta.past_len(), |mut log| {
                        log.extend(push.0.chars());
                        push.1
                    });
                if clear {
                    assert_eq!(drain.to_tuples(), []);
                } else {
                    assert_eq!(drain.to_tuples(), past_drain)
                }

                // without_past_drain
                let drain = self.without_past_drain.pre_update_drain(meta);
                assert_eq!(drain.future().to_tuples(), future_drain);
                self.without_past_drain.push(meta.past_len(), |mut log| {
                    log.extend(push.0.chars());
                    push.1
                });
            });
        }
        fn noop_forward_backward_log(&mut self) {
            self.meta.set_queue(RevQueue::RUN_NOT_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
        }
        fn forward_log(&mut self, get: Result<(String, char), ()>) {
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            match get {
                Ok(get) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);

                        // with_past_drain
                        let drain = self.with_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.all().to_tuples(), []);
                        assert_eq!(
                            self.with_past_drain
                                .forward_log()
                                .map(TransitionsLogIterMut::to_tuple),
                            Ok(get.clone())
                        );

                        // without_past_drain
                        let drain = self.without_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.future().to_tuples(), []);
                        assert_eq!(
                            self.without_past_drain
                                .forward_log()
                                .map(TransitionsLogIterMut::to_tuple),
                            Ok(get)
                        );
                    });
                }
                Err(()) => {
                    #[track_caller]
                    fn assert_err(log: &mut TransitionsLog<char, char>) {
                        assert_eq!(
                            log.forward_log().map(TransitionsLogIterMut::to_tuple),
                            Err(OutOfLog::caller())
                        );
                        log.clear_poison();
                    }

                    self.meta.update_ref(Ok(false), |_, _| ());
                    assert_err(&mut self.with_past_drain);
                    assert_err(&mut self.without_past_drain);
                }
            }
        }
        fn backward_log<const N: usize>(
            &mut self,
            future_drain: [(String, char); N],
            get: Result<(String, char), ()>,
        ) {
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            match get {
                Ok(get) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);

                        // with_past_drain
                        let mut drain = self.with_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.past().to_tuples(), []);
                        assert_eq!(drain.future().to_tuples(), future_drain);
                        assert_eq!(
                            self.with_past_drain
                                .backward_log()
                                .map(TransitionsLogIterMut::to_tuple),
                            Ok(get.clone())
                        );

                        // without_past_drain
                        let drain = self.without_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.future().to_tuples(), future_drain);
                        assert_eq!(
                            self.without_past_drain
                                .backward_log()
                                .map(TransitionsLogIterMut::to_tuple),
                            Ok(get)
                        );
                    });
                }
                Err(()) => {
                    assert_eq!(N, 0);
                    self.meta.update_ref(Ok(false), |_, _| ());

                    #[track_caller]
                    fn assert_err(log: &mut TransitionsLog<char, char>) {
                        assert_eq!(
                            log.backward_log().map(TransitionsLogIterMut::to_tuple),
                            Err(OutOfLog::caller())
                        );
                        log.clear_poison();
                    }

                    // with_past_drain
                    if self.with_past_drain.backward_log().is_ok() {
                        // Because this past-draining log secretly keeps one more transition than
                        // needed, OutOfLog will only be triggered if going backward twice past what
                        // the user might suspect to be in log.
                        // This is not true when clearing is involved however, which is why the
                        // first backward_log is not asserted to be Ok here.

                        // test again
                        assert_err(&mut self.with_past_drain);

                        // undoing first backward_log
                        assert!(self.with_past_drain.forward_log().is_ok());
                    } else {
                        self.with_past_drain.clear_poison();
                    }

                    // without_past_drain
                    assert_err(&mut self.without_past_drain);
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

        meta_and_logs.forward([], [], a(), false);
        meta_and_logs.forward([], [], b(), false);
        meta_and_logs.forward([], [], c(), false);
        meta_and_logs.forward([], [], d(), false);
        meta_and_logs.forward([], [], e(), false);
        meta_and_logs.forward([a()], [], f(), false);

        meta_and_logs.backward_log([], Ok(f()));
        meta_and_logs.backward_log([], Ok(e()));
        meta_and_logs.backward_log([], Ok(d()));
        meta_and_logs.backward_log([], Ok(c()));
        meta_and_logs.backward_log([], Err(())); // b() is unreachable but not yet drained

        meta_and_logs.forward_log(Ok(c()));
        meta_and_logs.forward_log(Ok(d()));
        meta_and_logs.forward_log(Ok(e()));
        meta_and_logs.forward_log(Ok(f()));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log([], Ok(f()));
        meta_and_logs.backward_log([], Ok(e()));

        meta_and_logs.forward([], [e(), f()], g(), false);

        meta_and_logs.backward_log([], Ok(g()));
        meta_and_logs.backward_log([], Ok(d()));
        meta_and_logs.backward_log([], Ok(c()));
        meta_and_logs.backward_log([], Err(()));

        meta_and_logs.forward_log(Ok(c()));
        meta_and_logs.forward_log(Ok(d()));
        meta_and_logs.forward_log(Ok(g()));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log([], Ok(g()));
        meta_and_logs.backward_log([], Ok(d()));

        meta_and_logs.forward([b(), c()], [d(), g()], h(), true);

        meta_and_logs.backward_log([], Ok(h()));
        meta_and_logs.backward_log([], Err(()));

        meta_and_logs.forward_log(Ok(h()));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.forward([], [], i(), false);

        meta_and_logs.backward_log([], Ok(i()));

        meta_and_logs.noop_forward_backward_log();

        meta_and_logs.backward_log([i()], Ok(h()));
        meta_and_logs.backward_log([], Err(()));
    }
}
