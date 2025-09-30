use crate::{
    log2::{OutOfLog, PreUpdateKind},
    meta::{RevDirection, RevMeta},
};
use bevy_log::error;
use core::{
    any::type_name,
    fmt::Debug,
    iter::{FusedIterator, Rev, Skip, Take},
    mem::take,
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

    truncated_future: usize,

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
            truncated_future: 0,
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

    pub fn push<const VALUE: bool>(
        &mut self,
        meta: &RevMeta,
        max_past_len: u64,
        transition: impl ValueOrClosure<T, VALUE>,
    ) {
        transition.push(self, meta, max_past_len);
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
            || self.index >= self.truncated_future.min(self.transitions.len())
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

pub trait ValueOrClosure<T, const VALUE: bool> {
    fn push(self, log: &mut TransitionLog<T>, meta: &RevMeta, max_past_len: u64);
}

impl<T> ValueOrClosure<T, true> for T {
    #[track_caller]
    fn push(self, log: &mut TransitionLog<T>, meta: &RevMeta, max_past_len: u64) {
        if log.meta_log_clears < meta.log_clears() {
            log.poison = Ok(());
            log.meta_log_clears = meta.log_clears();
            log.meta_log_exits = meta.log_exits();
            log.transitions.clear();
        } else if log.poison.is_err() {
            return poison_push_err::<T>();
        } else {
            log.meta_log_exits = meta.log_exits();
            log.transitions.truncate(log.index);
            let to_drain = (log.transitions.len() + 1).saturating_sub(max_past_len as usize);
            // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
            log.transitions.drain(..to_drain);
        }
        if max_past_len > 0 {
            log.transitions.push_back(self);
        }
        log.index = log.transitions.len();
        log.truncated_future = log.index;
    }
}

impl<T, F: for<'a> FnOnce(TransitionLogMut<'a, T>) -> T> ValueOrClosure<T, false> for F {
    #[track_caller]
    fn push(self, log: &mut TransitionLog<T>, meta: &RevMeta, max_past_len: u64) {
        let (past_drain, future_drain);
        if log.meta_log_clears < meta.log_clears() {
            log.poison = Ok(());
            log.meta_log_clears = meta.log_clears();
            log.meta_log_exits = meta.log_exits();
            past_drain = log.index;
            future_drain = log.transitions.len() - log.index;
        } else if log.poison.is_err() {
            return poison_push_err::<T>();
        } else {
            log.meta_log_exits = meta.log_exits();
            past_drain = (log.transitions.len() + 1)
                .saturating_sub((max_past_len as usize).saturating_add(1));
            future_drain = log.transitions.len() - log.index;
        }
        let mut buffer = [].into();
        let transition = self(TransitionLogMut {
            transitions: &mut log.transitions,
            buffer: &mut buffer,
            past_drain,
            future_drain,
        });
        log.transitions.reserve(buffer.len() + 1);
        log.transitions.extend(buffer);
        log.transitions.push_back(transition);
        log.index = log.transitions.len();
        log.truncated_future = log.index;
    }
}

#[track_caller]
fn poison_push_err<T>() {
    error!(
        "did not push `{}` to `TransitionLog` because log is poisoned",
        type_name::<T>()
    );
}

pub struct TransitionLogMut<'a, T> {
    transitions: &'a mut VecDeque<T>,
    buffer: &'a mut Box<[T]>,
    past_drain: usize,
    future_drain: usize,
}

impl<T> TransitionLogMut<'_, T> {
    /// Returns the number of transitions in the log.
    ///
    /// See [`VecDeque::len`].
    pub fn len(&self) -> usize {
        self.transitions.len() + self.buffer.len()
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
        self.transitions.is_empty() && self.buffer.is_empty()
    }

    /// Reserves capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::reserve`].
    pub fn reserve(&mut self, additional: usize) {
        self.transitions
            .reserve(additional.saturating_add(self.buffer.len()))
    }

    /// Reserves capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::reserve_exact`].
    pub fn reserve_exact(&mut self, additional: usize) {
        self.transitions
            .reserve_exact(additional.saturating_add(self.buffer.len()))
    }

    /// Tries to reserve capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::try_reserve`].
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions
            .try_reserve(additional.saturating_add(self.buffer.len()))
    }

    /// Tries to reserve capacity for at least `additional` more transitions.
    ///
    /// See [`VecDeque::try_reserve_exact`].
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions
            .try_reserve_exact(additional.saturating_add(self.buffer.len()))
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.transitions
            .shrink_to(min_capacity.saturating_add(self.buffer.len()))
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    pub fn shrink_to_fit(&mut self) {
        self.transitions.shrink_to(self.buffer.len())
    }

    pub fn drain_past(&mut self) -> Drain<'_, T> {
        let past_drain = self.past_drain;
        self.past_drain = 0;
        self.transitions.drain(..past_drain)
    }

    pub fn drain_future(&mut self) -> Drain<'_, T> {
        let future_drain = self.future_drain;
        self.future_drain = 0;
        self.transitions.drain(future_drain..)
    }

    pub fn drain_all(&mut self) -> TransitionAll<'_, T> {
        match (take(&mut self.past_drain), take(&mut self.future_drain)) {
            (past_drain, 0) => TransitionAll {
                drain: self.transitions.drain(..past_drain), // works with past_drain = 0 too
                buffer: self.buffer,
                past_drain: 0,
                keep: 0,
            },
            (0, future_drain) => {
                let offset = self.transitions.len() - future_drain;
                TransitionAll {
                    drain: self.transitions.drain(offset..),
                    buffer: self.buffer,
                    past_drain: 0,
                    keep: 0,
                }
            }
            (past_drain, future_drain) => {
                let keep = self.transitions.len() - past_drain - future_drain;
                TransitionAll {
                    drain: self.transitions.drain(..),
                    buffer: self.buffer,
                    past_drain,
                    keep,
                }
            }
        }
    }
}

impl<T> Drop for TransitionLogMut<'_, T> {
    fn drop(&mut self) {
        let truncate_future = self.transitions.len() - self.future_drain;
        self.transitions.truncate(truncate_future);
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..self.past_drain);
    }
}

pub struct TransitionAll<'a, T> {
    drain: Drain<'a, T>,
    buffer: &'a mut Box<[T]>,
    past_drain: usize,
    keep: usize,
}

impl<T> Iterator for TransitionAll<'_, T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.keep != 0 && self.past_drain == 0 {
            *self.buffer = self.drain.by_ref().take(self.keep).collect();
            self.keep = 0;
        }
        self.past_drain = self.past_drain.saturating_sub(1);
        self.drain.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<T> DoubleEndedIterator for TransitionAll<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.keep != 0 && self.drain.len() - self.past_drain == 0 {
            *self.buffer = self.drain.by_ref().take(self.keep).collect();
            self.keep = 0;
        }
        let transition = self.drain.next_back();
        self.past_drain = self.past_drain.min(self.drain.len());
        transition
    }
}

impl<T> ExactSizeIterator for TransitionAll<'_, T> {
    fn len(&self) -> usize {
        self.drain.len() - self.keep
    }
}

impl<T> FusedIterator for TransitionAll<'_, T> {}

impl<T> Drop for TransitionAll<'_, T> {
    fn drop(&mut self) {
        if self.keep != 0 {
            *self.buffer = self
                .drain
                .by_ref()
                .skip(self.past_drain)
                .take(self.keep)
                .collect();
        }
    }
}

/*
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
                    .push_drain_past(meta.past_len(), push)
                    .collect::<Vec<char>>();
                if clear {
                    assert_eq!(drained, []);
                } else {
                    assert_eq!(drained, past_drain)
                }

                // without_past_drain
                let drain = self.without_past_drain.pre_update_drain(meta);
                assert_eq!(drain.future().collect::<Vec<char>>(), future_drain);
                self.without_past_drain.push(meta.past_len(), push);
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
                        assert_eq!(log.forward_log(), Err(OutOfLog::caller()));
                        log.clear_poison();
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
                        assert_eq!(log.backward_log(), Err(OutOfLog::caller()));
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
}*/
