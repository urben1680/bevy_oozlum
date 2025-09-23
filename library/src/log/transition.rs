use crate::{
    log::{OutOfLog, PreUpdateKind},
    meta::{RevDirection, RevMeta},
};
use core::{
    fmt::Debug,
    iter::{FusedIterator, Skip, Take},
};
use std::collections::{
    TryReserveError, VecDeque,
    vec_deque::{Drain, Iter},
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
/// [`NOT_LOG`](crate::meta::RevDirection::NOT_LOG) one.
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
/// # use bevy::prelude::*;
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
///             log.push_and_truncate_past(meta.past_len(), new_transition);
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
/// # use bevy::prelude::*;
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
///             log.push_and_truncate_past(meta.past_len(), new_transition);
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
/// # use bevy::prelude::*;
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
///             let drain = log.push_and_drain_past(meta.past_len(), new_transition);
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
/// The `MyTransition` may for example contain an [entity ID](bevy::ecs::entity::Entity). This could be
/// the ID of a temporal entity that is associated to this transition. The cleanup then would be to
/// despawn it.
///
/// ```
/// # use bevy::prelude::*;
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
///             let drain = log.push_and_drain_past(meta.past_len(), new_transition);
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

    /// Contains the most recent global count of log exits that was witnessed.
    ///
    /// See [`RevMeta::log_exits`].
    meta_log_exits: u64,

    /// Contains the most recent global count of log clears that was witnessed.
    ///
    /// See [`RevMeta::log_clears`].
    meta_log_clears: u64,
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
            meta_log_exits: 0,
            meta_log_clears: 0,
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

    /// Pushes `transition` to the log and then shortens the log to contain at most `max_past_len`
    /// past values. If `0` is passed as the first argument, the pushed `transition` is immediately
    /// removed again.
    ///
    /// To receive an iterator over the removed values that got out-of-log, use
    /// [`push_and_drain_past`](Self::push_and_drain_past) instead. Note that the `max_past_len`
    /// logic works a bit differently with that method, see its documentation.
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) or
    /// [`pre_update_drain`](Self::pre_update_drain) **must** be called at least once in the
    /// [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    pub fn push_and_truncate_past(&mut self, max_past_len: u64, transition: T) {
        let to_drain = self.push_and_get_drain(max_past_len, transition);
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..to_drain);
    }

    /// Pushes `transition` to the log and then shortens the log to contain at most
    /// `max_past_len + 1` past values.
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    ///
    /// Before calling this, [`pre_update_drain`](Self::pre_update_drain) **must** be called at
    /// least once in the [present reversible frame](RevMeta::now). This method may panic if this
    /// was not done. Do not use [`pre_update`](Self::pre_update) because that method is needed as
    /// well to receive past values that are now out-of-log.
    ///
    /// This methods returns values that got out-of-log because the push of `transition` exceeded
    /// `max_past_len`. If there are no such values, the iterator is empty. `pre_update_drain`
    /// would also return values that got out-of-log when
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was queued and applied.
    ///
    /// The `max_past_len` addition by one is done to ensure that drained values were pushed at a
    /// frame that is now out-of-log. This may not be needed in some cases, but is important in
    /// others like when `T` contains an entity [ID](bevy::ecs::entity::Entity) that needs to remain
    /// alive as long it was spawned within the global log range. This makes this method less likely
    /// a source of bugs in the user logic.
    ///
    /// As an explaination why this log type would drain transitions earlier: Only the transition
    /// _from the very first state_ that can be reverted to _to the second state_ is required to be
    /// stored to cover a past length of `max_past_len`. The transition _to the first state_ is
    /// otherwise not useful because it would only be used to transition to and from the previous
    /// state that is now out-of-log.
    ///
    /// So if this addition by one was not done, the transition pushed at [`RevMeta::past_end`], a
    /// frame that is still in-log, would be drained here.
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    pub fn push_and_drain_past(
        &mut self,
        max_past_len: u64,
        transition: T,
    ) -> TransitionDrainAll<T> {
        let to_drain = self.push_and_get_drain(max_past_len + 1, transition);
        self.transitions.drain(..to_drain)
    }

    /// Pushes the `transition` and returns how many elements need to be removed from the past end
    /// of the log to contain at most `may_past_len` elements.
    fn push_and_get_drain(&mut self, max_past_len: u64, transition: T) -> usize {
        assert_eq!(
            self.index,
            self.transitions.len(),
            "`TransitionLog::pre_update(_drain)` was not called in advance"
        );
        self.transitions.push_back(transition);
        let to_drain = self.transitions.len().saturating_sub(max_past_len as usize);
        self.index = self.transitions.len() - to_drain;
        to_drain
    }

    /// Behaves like [`Self::push_and_drain_past`] except `max_past_len` is not increased by `1` and
    /// the returned iterator is not yet drained.
    ///
    /// This **must** be followed by [Self::drain_past].
    pub(super) fn push_and_iter_to_drain_past(
        &mut self,
        max_past_len: u64,
        transition: T,
    ) -> Iter<T> {
        assert_eq!(
            self.index,
            self.transitions.len(),
            "`TransitionsLog::pre_update(_drain)` was not called in advance"
        );
        self.transitions.push_back(transition);
        let to_drain = self.transitions.len().saturating_sub(max_past_len as usize);
        self.index = self.transitions.len() - to_drain;
        self.transitions.range(..to_drain)
    }

    /// Fully drains the log.
    pub(super) fn full_drain(&mut self) -> TransitionDrains<T> {
        let past_len = self.index;
        self.index = 0;
        TransitionDrains {
            transitions: self.transitions.drain(..),
            past_len,
        }
    }

    /// Drains the log entries that are in the future
    pub(super) fn drain_future(&mut self) -> TransitionDrains<T> {
        TransitionDrains {
            transitions: self.transitions.drain(self.index..),
            past_len: 0,
        }
    }

    /// Drains the `to_drain` log entries at the past end. The argument value is taken from
    /// [`Self::push_and_iter_to_drain_past`].
    pub(super) fn drain_past(&mut self, to_drain: usize) -> Drain<T> {
        self.transitions.drain(..to_drain)
    }

    /// Returns an empty drain, keeping the log unchanged.
    pub(super) fn empty_drain(&mut self) -> TransitionDrains<T> {
        TransitionDrains {
            transitions: self.transitions.drain(..0),
            past_len: 0,
        }
    }

    /// Truncates future log entries.
    pub(super) fn truncate_future(&mut self) {
        self.transitions.truncate(self.index);
    }

    /// Fully clears the log.
    pub(super) fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
    }

    /// Returns the transition value that was logged at the chronologically previous push. If the
    /// log is at the past end before this call, this method returns an [`OutOfLog`] error, leaving
    /// the log unchanged.
    ///
    /// The value can be mutated in case applying the transition is not only changing the state but
    /// also the transition itself. This may be needed if a previously added value is taken again
    /// and stored in this transition at `backward_log`. [`forward_log`](Self::forward_log) would
    /// then take the value from the transition to return it.
    ///
    /// This is used during [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog).
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) or
    /// [`pre_update_drain`](Self::pre_update_drain) **must** be called at least once in the
    /// [present reversible frame](RevMeta::now).
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    #[track_caller]
    pub fn backward_log(&mut self) -> Result<&mut T, OutOfLog> {
        let index = self.index.checked_sub(1).ok_or(OutOfLog::new())?;

        // self.index should always be <= the deque len, so successfully reducing the index without
        // underflow is expected to result in a valid index into the log. If this is not the case
        // here, this would be a crate bug.
        let transition = self.transitions.get_mut(index).unwrap();

        self.index = index;
        Ok(transition)
    }

    /// Returns the transition value that was logged at the chronologically next push. If the
    /// log is at the past end before this call, this method returns an [`OutOfLog`] error, leaving
    /// the log unchanged.
    ///
    /// The value can be mutated in case applying the transition is not only changing the state but
    /// also the transition itself. This may be needed if a previously added value is taken again
    /// and stored in this transition at [`backward_log`](Self::backward_log). `forward_log` would
    /// then take the value from the transition to return it.
    ///
    /// This is used during [`RevDirection::FORWARD_LOG`](crate::meta::RevDirection::FORWARD_LOG).
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) or
    /// [`pre_update_drain`](Self::pre_update_drain) **must** be called at least once in the
    /// [present reversible frame](RevMeta::now).
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    #[track_caller]
    pub fn forward_log(&mut self) -> Result<&mut T, OutOfLog> {
        self.transitions
            .get_mut(self.index)
            .inspect(|_| self.index += 1)
            .ok_or(OutOfLog::new())
    }

    /// Checks which [`RevMeta`] state was observed previously and if the log needs to adjust itself
    /// to changes after that. Besides [`Self::meta_log_exits`] and [`Self::meta_log_clears`], no
    /// such adjustments are done to the log yet and need a follow-up according to the returned
    /// [`PreUpdateKind`].
    pub(super) fn pre_update_kind(&mut self, meta: &RevMeta) -> PreUpdateKind {
        if self.meta_log_clears < meta.log_clears() {
            self.meta_log_clears = meta.log_clears();
            self.meta_log_exits = meta.log_exits();
            PreUpdateKind::RemoveLog
        } else if self.meta_log_exits < meta.log_exits() {
            self.meta_log_exits = meta.log_exits();
            PreUpdateKind::RemoveFuture
        } else if meta
            .get_running_direction()
            .is_some_and(RevDirection::is_not_log)
        {
            PreUpdateKind::RemoveFuture
        } else {
            PreUpdateKind::Nothing
        }
    }

    /// This method **must** be called once per [reversible frame](RevMeta::now) before any other
    /// mutation.
    ///
    /// This may remove log entries. If a draining iterator for these entries is required, use
    /// [`pre_update_drain`](Self::pre_update_drain) instead.
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    pub fn pre_update(&mut self, meta: &RevMeta) {
        match self.pre_update_kind(meta) {
            PreUpdateKind::RemoveLog => self.clear(),
            PreUpdateKind::RemoveFuture => self.truncate_future(),
            PreUpdateKind::Nothing => {}
        }
    }

    /// This method **must** be called once per [reversible frame](RevMeta::now) before any other
    /// mutation.
    ///
    /// This may remove log entries which can be received with the returned iterators. The iterators
    /// may be empty. If no iterators are required, use [`pre_update`](Self::pre_update) instead.
    ///
    /// For examples, see the [type level documentation](TransitionLog).
    pub fn pre_update_drain<'log, 'm>(
        &'log mut self,
        meta: &'m RevMeta,
    ) -> TransitionDrains<'log, T> {
        match self.pre_update_kind(meta) {
            PreUpdateKind::RemoveLog => self.full_drain(),
            PreUpdateKind::RemoveFuture => self.drain_future(),
            PreUpdateKind::Nothing => self.empty_drain(),
        }
    }
}

/// Contains iterators for the past and future log entries that need to be removed at this frame.
pub struct TransitionDrains<'log, T> {
    pub(super) transitions: Drain<'log, T>,
    pub(super) past_len: usize,
}

impl<'log, T> TransitionDrains<'log, T> {
    /// Drains all transitions that are so far in the past that they are out-of-log.
    ///
    /// The transitions are returned in the order they were pushed.
    ///
    /// Calling this method a second time will return an iterator that continues where the first
    /// usage ended. If the iterator at the first time was exhausted, following ones will be empty.
    ///
    /// See [`VecDeque::drain`].
    pub fn past<'a>(&'a mut self) -> TransitionDrainPast<'a, 'log, T> {
        TransitionDrainPast::new(&mut self.transitions, &mut self.past_len)
    }

    /// Drains all transitions that are in the future and thus became out-of-log.
    ///
    /// The transitions are returned in the order they were pushed.
    ///
    /// This consumes `self`. If the past transitions are of interest, use [`past`](Self::past)
    /// first.
    ///
    /// See [`VecDeque::drain`].
    pub fn future(self) -> TransitionDrainFuture<'log, T> {
        self.transitions.skip(self.past_len)
    }

    /// Drains all transitions that are so far in the past that they are out-of-log, or are in the
    /// future and thus became out-of-log.
    ///
    /// The transitions are returned in the order they were pushed.
    ///
    /// If the past and future transitions need to be separated or one or the other are not of
    /// interest, use [`past`](Self::past) and/or [`future`](Self::future).
    ///
    /// If this is called after [`past`](Self::past), this is equivalent to
    /// [`future`](Self::future).
    ///
    /// See [`VecDeque::drain`].
    pub fn all(self) -> TransitionDrainAll<'log, T> {
        self.transitions
    }
}

/// Draining transition iterator returned by [`TransitionDrains::past`].
// do not implement DoubleEndedIterator because using that would purge the non-taken future values!
pub struct TransitionDrainPast<'a, 'log, T> {
    transitions: Take<&'a mut Drain<'log, T>>,
    past_len: &'a mut usize,
}

impl<'a, 'log, T> TransitionDrainPast<'a, 'log, T> {
    pub(super) fn new(transitions: &'a mut Drain<'log, T>, past_len: &'a mut usize) -> Self {
        let transitions = transitions.take(*past_len);
        Self {
            transitions,
            past_len,
        }
    }
}

impl<'a, 'log, T> Iterator for TransitionDrainPast<'a, 'log, T> {
    type Item = T;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let out = self.transitions.next();
        *self.past_len = self.transitions.len();
        out
    }

    #[inline]
    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        let out = self.transitions.nth(n);
        *self.past_len = self.transitions.len();
        out
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.transitions.size_hint()
    }

    #[inline]
    fn fold<B, F>(self, init: B, f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, Self::Item) -> B,
    {
        *self.past_len = 0;
        self.transitions.fold(init, f)
    }

    #[inline]
    fn for_each<F>(self, f: F)
    where
        Self: Sized,
        F: FnMut(Self::Item),
    {
        *self.past_len = 0;
        self.transitions.for_each(f);
    }
}

impl<'a, 'log, T> ExactSizeIterator for TransitionDrainPast<'a, 'log, T> {
    #[inline]
    fn len(&self) -> usize {
        self.transitions.len()
    }
}

impl<'a, 'log, T> FusedIterator for TransitionDrainPast<'a, 'log, T> {}

/// Draining transition iterator returned by [`TransitionDrains::future`].
pub type TransitionDrainFuture<'log, T> = Skip<Drain<'log, T>>;

/// Draining transition iterator returned by [`TransitionDrains::all`].
pub type TransitionDrainAll<'log, T> = Drain<'log, T>;

#[cfg(test)]
mod test {
    use core::num::NonZeroU64;

    use crate::meta::RevQueue;

    use super::*;

    struct MetaAndLogs {
        meta: RevMeta,
        with_past_drain: TransitionLog<char>,
        without_past_drain: TransitionLog<char>,
    }

    impl MetaAndLogs {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(NonZeroU64::new(max_world_states), false),
                with_past_drain: TransitionLog::new(),
                without_past_drain: TransitionLog::new(),
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

                // with_past_drain
                let mut drain = self.with_past_drain.pre_update_drain(meta);
                if clear {
                    assert_eq!(drain.past().collect::<Vec<char>>(), past_drain);
                } else {
                    assert_eq!(drain.past().collect::<Vec<char>>(), []);
                }
                assert_eq!(drain.future().collect::<Vec<char>>(), future_drain);
                let drained = self
                    .with_past_drain
                    .push_and_drain_past(meta.past_len(), push)
                    .collect::<Vec<char>>();
                if clear {
                    assert_eq!(drained, []);
                } else {
                    assert_eq!(drained, past_drain)
                }

                // without_past_drain
                let drain = self.without_past_drain.pre_update_drain(meta);
                assert_eq!(drain.future().collect::<Vec<char>>(), future_drain);
                self.without_past_drain
                    .push_and_truncate_past(meta.past_len(), push);
            });
        }
        fn noop_forward_backward_log(&mut self) {
            self.meta.set_queue(RevQueue::RUN_NOT_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
        }
        fn forward_log(&mut self, get: Result<char, ()>) {
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            match get {
                Ok(get) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);

                        // with_past_drain
                        let drain = self.with_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.all().collect::<Vec<char>>(), []);
                        assert_eq!(
                            self.with_past_drain.forward_log().map(|char| *char),
                            Ok(get)
                        );

                        // without_past_drain
                        let drain = self.without_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.future().collect::<Vec<char>>(), []);
                        assert_eq!(
                            self.without_past_drain.forward_log().map(|char| *char),
                            Ok(get)
                        );
                    });
                }
                Err(()) => {
                    self.meta.update_ref(Ok(false), |_, _| ());

                    #[track_caller]
                    fn assert_err(log: &mut TransitionLog<char>) {
                        assert_eq!(log.forward_log(), Err(OutOfLog::new()));
                    }

                    assert_err(&mut self.with_past_drain);
                    assert_err(&mut self.without_past_drain);
                }
            }
        }
        fn backward_log<const N: usize>(&mut self, future_drain: [char; N], get: Result<char, ()>) {
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            match get {
                Ok(get) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);

                        // with_past_drain
                        let mut drain = self.with_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.past().collect::<Vec<char>>(), []);
                        assert_eq!(drain.future().collect::<Vec<char>>(), future_drain);
                        assert_eq!(
                            self.with_past_drain.backward_log().map(|char| *char),
                            Ok(get)
                        );

                        // without_past_drain
                        let drain = self.without_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.future().collect::<Vec<char>>(), future_drain);
                        assert_eq!(
                            self.without_past_drain.backward_log().map(|char| *char),
                            Ok(get)
                        );
                    });
                }
                Err(()) => {
                    assert_eq!(N, 0);
                    self.meta.update_ref(Ok(false), |_, _| ());

                    #[track_caller]
                    fn assert_err(log: &mut TransitionLog<char>) {
                        assert_eq!(log.backward_log(), Err(OutOfLog::new()));
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

        meta_and_logs.forward([], [], 'a', false);
        meta_and_logs.forward([], [], 'b', false);
        meta_and_logs.forward([], [], 'c', false);
        meta_and_logs.forward([], [], 'd', false);
        meta_and_logs.forward([], [], 'e', false);
        meta_and_logs.forward(['a'], [], 'f', false);

        meta_and_logs.backward_log([], Ok('f'));
        meta_and_logs.backward_log([], Ok('e'));
        meta_and_logs.backward_log([], Ok('d'));
        meta_and_logs.backward_log([], Ok('c'));
        meta_and_logs.backward_log([], Err(())); // 'b' is unreachable but not yet drained

        meta_and_logs.forward_log(Ok('c'));
        meta_and_logs.forward_log(Ok('d'));
        meta_and_logs.forward_log(Ok('e'));
        meta_and_logs.forward_log(Ok('f'));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log([], Ok('f'));
        meta_and_logs.backward_log([], Ok('e'));

        meta_and_logs.forward([], ['e', 'f'], 'g', false);

        meta_and_logs.backward_log([], Ok('g'));
        meta_and_logs.backward_log([], Ok('d'));
        meta_and_logs.backward_log([], Ok('c'));
        meta_and_logs.backward_log([], Err(()));

        meta_and_logs.forward_log(Ok('c'));
        meta_and_logs.forward_log(Ok('d'));
        meta_and_logs.forward_log(Ok('g'));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.backward_log([], Ok('g'));
        meta_and_logs.backward_log([], Ok('d'));

        meta_and_logs.forward(['b', 'c'], ['d', 'g'], 'h', true);

        meta_and_logs.backward_log([], Ok('h'));
        meta_and_logs.backward_log([], Err(()));

        meta_and_logs.forward_log(Ok('h'));
        meta_and_logs.forward_log(Err(()));

        meta_and_logs.forward([], [], 'i', false);

        meta_and_logs.backward_log([], Ok('i'));

        meta_and_logs.noop_forward_backward_log();

        meta_and_logs.backward_log(['i'], Ok('h'));
        meta_and_logs.backward_log([], Err(()));
    }
}
