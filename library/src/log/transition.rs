use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque, vec_deque::Iter};

use crate::{
    log::PreUpdateVariant,
    meta::{RevDirection, RevMeta},
};

use super::OutOfLog;

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
/// These methods make sure the log catches such situations even if the current system does not run
/// in the very same frame they took effect, like when that frame was missed because of run
/// conditions.
///
/// If the log is mutated multiple times per frame, these `pre_update` methods only need to called
/// once at the beginning.
///
/// After that, depending on the direction, you either push a new transition into the log or
/// traverse it forwards or backwards.
///
/// This log alone is only suited for a constant amount of updates per frame. For a variable amount
/// of updates, consider pairing it with a [`PastLenLog`](crate::log::PastLenLog).
///
/// ## Basic `Local` usage
///
/// Usually transition types are just plain data the log can truncate when it is no longer needed.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<TransitionLog<MyType>>) -> Result<(), BevyError> {
///     log.pre_update(&meta);
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///
///             // mutate some state with the new transition
///
///             // push transition to the log
///             log.push_and_truncate_past(meta.past_len(), new_transition);
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///
///             // mutate some state with the logged transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
///
///             // mutate some state with the logged transition
///         }
///     }
///
///     Ok(())
/// }
/// ```
///
/// ## Constant amount of updates > 1
///
/// If a constant amount of transitions needs to be stores per update, multiply the `past_len`
/// value.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<TransitionLog<MyType>>) -> Result<(), BevyError> {
///     log.pre_update(&meta);
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition_a = todo!();
///             let new_transition_b = todo!();
///
///             // mutate some state with the new transitions
///
///             // push the transitions to the log
///             let max_past_len = meta.past_len() * 2;
///             log.push_and_truncate_past(max_past_len, new_transition_a);
///             log.push_and_truncate_past(max_past_len, new_transition_b);
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition_a = log.forward_log()?;
///             let next_transition_b = log.forward_log()?;
///
///             // mutate some state with the logged transitions
///         },
///         RevDirection::BackwardLog => {
///             // note that the order of transitions is reversed here
///             let previous_transition_b = log.backward_log()?;
///             let previous_transition_a = log.backward_log()?;
///
///             // mutate some state with the logged transitions
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
/// longer needs should be drained, not immediately dropped.
///
/// In this example the cleanup is needed for log entries in the future part.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<TransitionLog<MyType>>) -> Result<(), BevyError> {
///     for future_transition in log.pre_update_drain(&meta).future() {
///         // do cleanup tasks with future transitions
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///
///             // mutate some state with the new transition
///
///             // push the transition to the log
///             log.push_and_truncate_past(meta.past_len(), new_transition);
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///
///             // mutate some state with the logged transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
///
///             // mutate some state with the logged transition
///         }
///     }
///
///     Ok(())
/// }
/// ```
///
/// ## Draining past
///
/// Like the _Draining future_ example, but here the relevant transitions for cleanup work are the
/// past part of the log.
///
/// There are two iterations in the example where cleanup work has to happen. This is because either
/// of the two could happen in isolation, the first because
/// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was queued and applied and the second because
/// the log exceeds [`RevMeta::past_len`].
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<TransitionLog<MyType>>) -> Result<(), BevyError> {
///     for past_transition in log.pre_update_drain(&meta).past() {
///         // do cleanup tasks with past transitions
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///
///             // mutate some state with the new transition
///
///             // push the transition to the log
///             let iter = log.push_and_drain_past(meta.past_len(), new_transition);
///
///             for past_transition in iter {
///                 // do cleanup tasks with past transitions
///             }
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///
///             // mutate some state with the logged transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
///
///             // mutate some state with the logged transition
///         }
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
/// The `MyType` may for example contain an [ID](bevy::ecs::entity::Entity) temporal entity for this
/// transition that needs to be despawned.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<TransitionLog<MyType>>) -> Result<(), BevyError> {
///     let mut drain = log.pre_update_drain(&meta);
///
///     for past_transition in drain.past() {
///         // do cleanup tasks with past transitions
///     }
///
///     for future_transition in drain.future() {
///         // do cleanup tasks with future transitions
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///
///             // mutate some state with the new transition
///
///             // push the transition to the log
///             let iter = log.push_and_drain_past(meta.past_len(), new_transition);
///
///             for past_transition in iter {
///                 // do cleanup tasks with past transitions
///             }
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///
///             // mutate some state with the logged transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
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
    transitions: VecDeque<T>,
    index: usize,
    global_log_clears: u64,
    global_log_exits: u64,
}

impl<T> Default for TransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> TransitionLog<T> {
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            index: 0,
            global_log_clears: 0,
            global_log_exits: 0,
        }
    }

    /// Creates an empty log with space for at least `capacity` transitions.
    ///
    /// See [`VecDeque::with_capacity`].
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(capacity),
            index: 0,
            global_log_clears: 0,
            global_log_exits: 0,
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

    pub fn push_and_truncate_past(&mut self, max_past_len: u64, transition: T) {
        let to_drain = self.push_and_get_drain(max_past_len, transition);
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..to_drain);
    }
    pub fn push_and_drain_past(&mut self, max_past_len: u64, transition: T) -> TransitionDrain<T> {
        // todo: explain + 1 in regard of footgun for usecase
        let to_drain = self.push_and_get_drain(max_past_len + 1, transition);
        self.transitions.drain(..to_drain)
    }
    fn push_and_get_drain(&mut self, max_past_len: u64, transition: T) -> usize {
        assert_eq!(self.index, self.transitions.len()); // do not truncate here, call pre_update!
        self.transitions.push_back(transition);
        // todo: explain + 1 in regard of footgun for usecase
        let to_drain = self.transitions.len().saturating_sub(max_past_len as usize);
        self.index = self.transitions.len() - to_drain;
        to_drain
    }
    pub(super) fn push_and_iter_to_drain_past(
        &mut self,
        max_past_len: u64,
        transition: T,
    ) -> Iter<T> {
        assert_eq!(self.index, self.transitions.len()); // do not truncate here, call pre_update!
        self.transitions.push_back(transition);
        let to_drain = self.transitions.len().saturating_sub(max_past_len as usize);
        self.index = self.transitions.len() - to_drain;
        self.transitions.range(..to_drain)
    }
    pub(super) fn full_drain(&mut self) -> TransitionDrains<T> {
        let past_len = self.index;
        self.index = 0;
        TransitionDrains {
            transitions: self.transitions.drain(..),
            past_len,
        }
    }
    pub(super) fn drain_future(&mut self) -> TransitionDrains<T> {
        TransitionDrains {
            transitions: self.transitions.drain(self.index..),
            past_len: 0,
        }
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> TransitionDrain<T> {
        self.transitions.drain(..to_drain)
    }
    pub(super) fn empty_drain(&mut self) -> TransitionDrains<T> {
        TransitionDrains {
            transitions: self.transitions.drain(..0),
            past_len: 0,
        }
    }
    pub(super) fn truncate_future(&mut self) {
        self.transitions.truncate(self.index);
    }
    pub(super) fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<&mut T, OutOfLog> {
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;

        // self.index should always be <= the deque len, so successfully reducing it without
        // underflow is expected to result in a valid index into the log. If this is not the case
        // here, the log was in an invalid state before calling the current method, this would be a
        // crate bug.
        let transition = self.transitions.get_mut(index).unwrap();

        self.index = index;
        Ok(transition)
    }
    pub fn forward_log(&mut self) -> Result<&mut T, OutOfLog> {
        self.transitions
            .get_mut(self.index)
            .inspect(|_| self.index += 1)
            .ok_or(OutOfLog)
    }
    pub(super) fn pre_update_check(&mut self, meta: &RevMeta) -> PreUpdateVariant {
        if self.global_log_clears < meta.log_clears() {
            self.global_log_clears = meta.log_clears();
            self.global_log_exits = meta.log_exits();
            PreUpdateVariant::RemoveLog
        } else if self.global_log_exits < meta.log_exits() {
            self.global_log_exits = meta.log_exits();
            PreUpdateVariant::RemoveFuture
        } else if meta
            .get_running_direction()
            .is_some_and(RevDirection::is_not_log)
        {
            PreUpdateVariant::RemoveFuture
        } else {
            PreUpdateVariant::Nothing
        }
    }
    /// Call this method once per frame before every other mutation. It may be skipped if:
    ///
    /// 1. This log is updated every time [`RevUpdate`](crate::schedule::RevUpdate) runs **and**
    /// 2. [`RevMeta::queue_clear`] is not used
    pub fn pre_update(&mut self, meta: &RevMeta) {
        match self.pre_update_check(meta) {
            PreUpdateVariant::RemoveLog => self.clear(),
            PreUpdateVariant::RemoveFuture => self.truncate_future(),
            PreUpdateVariant::Nothing => {}
        }
    }
    /// Call this method once per frame before every other mutation. It may be skipped if:
    ///
    /// 1. This log is updated every time [`RevUpdate`](crate::schedule::RevUpdate) runs **and**
    /// 2. [`RevMeta::queue_clear`] is not used
    pub fn pre_update_drain<'log, 'm>(
        &'log mut self,
        meta: &'m RevMeta,
    ) -> TransitionDrains<'log, T> {
        match self.pre_update_check(meta) {
            PreUpdateVariant::RemoveLog => self.full_drain(),
            PreUpdateVariant::RemoveFuture => self.drain_future(),
            PreUpdateVariant::Nothing => self.empty_drain(),
        }
    }
}

pub struct TransitionDrains<'log, T> {
    pub(super) transitions: TransitionDrain<'log, T>,
    pub(super) past_len: usize,
}

pub type TransitionDrainPast<'a, 'log, T> = core::iter::Take<&'a mut TransitionDrain<'log, T>>;

pub type TransitionDrainFuture<'log, T> = core::iter::Skip<TransitionDrain<'log, T>>;

pub type TransitionDrain<'log, T> = std::collections::vec_deque::Drain<'log, T>;

impl<'log, T> TransitionDrains<'log, T> {
    pub fn past<'a>(&'a mut self) -> TransitionDrainPast<'a, 'log, T> {
        let past_len = self.past_len;
        self.past_len = 0;
        self.transitions.by_ref().take(past_len)
    }
    pub fn future(self) -> TransitionDrainFuture<'log, T> {
        self.transitions.skip(self.past_len)
    }
    pub fn all(self) -> TransitionDrain<'log, T> {
        self.transitions
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU64;

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
        fn forward_log(&mut self, get: Result<char, OutOfLog>) {
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
                Err(OutOfLog) => {
                    self.meta.update_ref(Ok(false), |_, _| ());
                    assert_eq!(self.with_past_drain.forward_log(), Err(OutOfLog));
                    assert_eq!(self.without_past_drain.forward_log(), Err(OutOfLog));
                }
            }
        }
        fn backward_log<const N: usize>(
            &mut self,
            future_drain: [char; N],
            get: Result<char, OutOfLog>,
        ) {
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
                Err(OutOfLog) => {
                    assert_eq!(N, 0);
                    self.meta.update_ref(Ok(false), |_, _| ());

                    // with_past_drain
                    if self.with_past_drain.backward_log().is_ok() {
                        // Because this past-draining log secretly keeps one more transition than
                        // needed, OutOfLog will only be triggered if going backward twice past what
                        // the user might suspect to be in log.
                        // This is not true when clearing is involved however, which is why the
                        // first backward_log is not asserted to be Ok here.

                        // test again
                        assert_eq!(self.with_past_drain.backward_log(), Err(OutOfLog));

                        // undoing first backward_log
                        assert!(self.with_past_drain.forward_log().is_ok());
                    }

                    // without_past_drain
                    assert_eq!(self.without_past_drain.backward_log(), Err(OutOfLog));
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
        meta_and_logs.backward_log([], Err(OutOfLog)); // 'b' is unreachable but not yet drained

        meta_and_logs.forward_log(Ok('c'));
        meta_and_logs.forward_log(Ok('d'));
        meta_and_logs.forward_log(Ok('e'));
        meta_and_logs.forward_log(Ok('f'));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log([], Ok('f'));
        meta_and_logs.backward_log([], Ok('e'));

        meta_and_logs.forward([], ['e', 'f'], 'g', false);

        meta_and_logs.backward_log([], Ok('g'));
        meta_and_logs.backward_log([], Ok('d'));
        meta_and_logs.backward_log([], Ok('c'));
        meta_and_logs.backward_log([], Err(OutOfLog));

        meta_and_logs.forward_log(Ok('c'));
        meta_and_logs.forward_log(Ok('d'));
        meta_and_logs.forward_log(Ok('g'));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log([], Ok('g'));
        meta_and_logs.backward_log([], Ok('d'));

        meta_and_logs.forward(['b', 'c'], ['d', 'g'], 'h', true);

        meta_and_logs.backward_log([], Ok('h'));
        meta_and_logs.backward_log([], Err(OutOfLog));

        meta_and_logs.forward_log(Ok('h'));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.forward([], [], 'i', false);

        meta_and_logs.backward_log([], Ok('i'));

        meta_and_logs.noop_forward_backward_log();

        meta_and_logs.backward_log(['i'], Ok('h'));
        meta_and_logs.backward_log([], Err(OutOfLog));
    }
}
