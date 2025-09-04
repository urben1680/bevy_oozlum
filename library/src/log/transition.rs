use core::fmt::Debug;
use std::collections::{
    VecDeque,
    vec_deque::{Drain, Iter},
};

use crate::{
    log::PreUpdateVariant,
    meta::{RevDirection, RevMeta},
};

use super::{INDEX_OOB, OutOfLog};

/// A simple log that contains exactly one transition type `T` per update.
///
/// # Examples
///
/// Generally, a variant of [`DenseTransitionLog::pre_update`] needs to be called before any other
/// mutation. These methods handle when [`RevMeta::queue_clear`] was used or the
/// [`RevDirection`](crate::meta::RevDirection) changed from a log variant to the `NOT_LOG` one.
///
/// These methods make sure the log catches such situations even if the current system does not run
/// in the very same frame they take effect, like because of run conditions.
///
/// If you mutate the log multiple times per frame, you only need to call these once at the
/// beginning.
///
/// After that, depending on the direction, you either push a new transition into the log or
/// traverse it forwards or backwards.
///
/// Like all transition logs, this one is only suited for a constant amount of updates per frame.
/// For a variable amount of updates, consider pairing it with a
/// [`PastLenLog`](crate::log::PastLenLog).
///
/// ## Basic `Local` usage
///
/// Usually transition types are just plain data the log can truncate when it is no longer needed.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseTransitionLog;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<DenseTransitionLog<MyType>>) -> Result<(), BevyError> {
///     // always call before any mutation
///     log.pre_update(&meta);
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///             // mutate some state with the new transition
///
///             // push transition to the log
///             log.push_and_truncate_past(meta.past_len(), new_transition);
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///             // mutate some state with the new transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
///             // mutate some state with the new transition
///         }
///     }
/// }
/// ```
///
/// ## Draining future
///
/// There may be cases where you need to do some sort of cleanup in which case the transitions that
/// the log no longer needs should be drained, not immediately dropped.
///
/// In this example the cleanup is needed for log entries in the future part.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseTransitionLog;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<DenseTransitionLog<MyType>>) -> Result<(), BevyError> {
///     // always call before any mutation
///     let iter = log.pre_update_drain_past(&meta);
///
///     for future_transition in iter {
///         // do cleanup tasks
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///             // mutate some state with the new transition
///
///             // push transition to the log
///             log.push_and_truncate_past(meta.past_len(), new_transition);
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///             // mutate some state with the new transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
///             // mutate some state with the new transition
///         }
///     }
/// }
/// ```
///
/// ## Draining past
///
/// Like the _Draining future_ example, but here the relevant transitions for cleanup work are the
/// past part of the log.
///
/// There are two iterations in the example where cleanup work has to happen. This is because either
/// of the two could happen in isolation, the first because [`RevMeta::queue_clear`] has been used
/// and the second because the log reached [`RevMeta::past_len`].
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseTransitionLog;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<DenseTransitionLog<MyType>>) -> Result<(), BevyError> {
///     // always call before any mutation
///     let iter = log.pre_update_drain_past(&meta);
///
///     for past_transition in iter {
///         // do cleanup tasks
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///             // mutate some state with the new transition
///
///             // push transition to the log
///             let iter = log.push_and_drain_past(meta.past_len(), new_transition);
///
///             for past_transition in iter {
///                 // do cleanup tasks with past transitions
///             }
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///             // mutate some state with the new transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
///             // mutate some state with the new transition
///         }
///     }
/// }
/// ```
///
/// ## Drain future and past
///
/// Combination of the two examples above. The distinction between future and past transitions is
/// of course optional.
///
/// The `MyType` may for example contain an [`Entity`](bevy::ecs::entity::Entity) ID of a temporal
/// entity for this transition that needs to be despawned.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_oozlum::prelude::*;
/// # use bevy_oozlum::log::DenseTransitionLog;
/// # struct MyType;
/// fn system(meta: Res<RevMeta>, mut log: Local<DenseTransitionLog<MyType>>) -> Result<(), BevyError> {
///     // always call before any mutation
///     let (mut iter, past_len) = log.pre_update_drain(&meta);
///
///     for past_transition in iter.by_ref().take(past_len) {
///         // do cleanup tasks with past transitions
///     }
///
///     for future_transition in iter {
///         // do cleanup tasks with future transitions
///     }
///
///     match meta.running_direction() {
///         RevDirection::NOT_LOG => {
///             let new_transition = todo!();
///             // mutate some state with the new transition
///
///             // push transition to the log
///             let iter = log.push_and_drain_past(meta.past_len(), new_transition);
///
///             for past_transition in iter {
///                 // do cleanup tasks with past transitions
///             }
///         },
///         RevDirection::FORWARD_LOG => {
///             let next_transition = log.forward_log()?;
///             // mutate some state with the new transition
///         },
///         RevDirection::BackwardLog => {
///             let previous_transition = log.backward_log()?;
///             // mutate some state with the new transition
///         }
///     }
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
    pub fn push_and_truncate_past(&mut self, max_past_len: usize, transition: T) {
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.push_and_drain_past(max_past_len, transition);
    }
    pub fn push_and_drain_past(&mut self, max_past_len: usize, transition: T) -> Drain<T> {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        // todo: explain + 1 in regard of footgun for usecase
        let to_drain = self.transitions.len().saturating_sub(max_past_len + 1);
        self.index = self.transitions.len() - to_drain;
        self.transitions.drain(..to_drain)
    }
    pub(super) fn push_and_iter_to_drain_past(
        &mut self,
        max_past_len: usize,
        transition: T,
    ) -> Iter<T> {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        let to_drain = self.transitions.len().saturating_sub(max_past_len + 1);
        self.index = self.transitions.len() - to_drain;
        self.transitions.range(..to_drain)
    }
    pub(super) fn drain_future(&mut self) -> Drain<T> {
        self.transitions.drain(self.index..)
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> Drain<T> {
        self.transitions.drain(..to_drain)
    }
    pub(super) fn truncate_future(&mut self) {
        self.transitions.truncate(self.index);
    }
    pub(super) fn truncate_past(&mut self) {
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..self.index);
        self.index = 0;
    }
    pub(super) fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
    }
    pub(super) fn empty_drain(&mut self) -> Drain<T> {
        self.transitions.drain(..0)
    }
    pub(super) fn full_drain(&mut self) -> (Drain<T>, usize) {
        let past_len = self.index;
        self.index = 0;
        (self.transitions.drain(..), past_len)
    }
    pub fn backward_log(&mut self) -> Result<&mut T, OutOfLog> {
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        let transition = self.transitions.get_mut(index).expect(INDEX_OOB);
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
            PreUpdateVariant::DropLog
        } else if self.global_log_exits < meta.log_exits() {
            self.global_log_exits = meta.log_exits();
            PreUpdateVariant::DropFuture
        } else if meta
            .get_running_direction()
            .is_some_and(RevDirection::is_not_log)
        {
            PreUpdateVariant::DropFuture
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
            PreUpdateVariant::DropLog => self.clear(),
            PreUpdateVariant::DropFuture => self.truncate_future(),
            PreUpdateVariant::Nothing => {}
        }
    }
    /// Call this method once per frame before every other mutation. It may be skipped if:
    ///
    /// 1. This log is updated every time [`RevUpdate`](crate::schedule::RevUpdate) runs **and**
    /// 2. [`RevMeta::queue_clear`] is not used
    pub fn pre_update_drain_past<'a, 'm>(&'a mut self, meta: &'m RevMeta) -> Drain<'a, T> {
        match self.pre_update_check(meta) {
            PreUpdateVariant::DropLog => {
                self.truncate_future();
                self.index = 0;
                return self.transitions.drain(..);
            }
            PreUpdateVariant::DropFuture => self.truncate_future(),
            PreUpdateVariant::Nothing => {}
        }
        self.empty_drain()
    }
    /// Call this method once per frame before every other mutation. It may be skipped if:
    ///
    /// 1. This log is updated every time [`RevUpdate`](crate::schedule::RevUpdate) runs **and**
    /// 2. [`RevMeta::queue_clear`] is not used
    pub fn pre_update_drain_future<'a, 'm>(&'a mut self, meta: &'m RevMeta) -> Drain<'a, T> {
        match self.pre_update_check(meta) {
            PreUpdateVariant::DropLog => {
                self.truncate_past();
                self.transitions.drain(..)
            }
            PreUpdateVariant::DropFuture => self.transitions.drain(self.index..),
            PreUpdateVariant::Nothing => self.empty_drain(),
        }
    }
    /// Call this method once per frame before every other mutation. It may be skipped if:
    ///
    /// 1. This log is updated every time [`RevUpdate`](crate::schedule::RevUpdate) runs **and**
    /// 2. [`RevMeta::queue_clear`] is not used
    pub fn pre_update_drain<'a, 'm>(&'a mut self, meta: &'m RevMeta) -> (Drain<'a, T>, usize) {
        match self.pre_update_check(meta) {
            PreUpdateVariant::DropLog => self.full_drain(),
            PreUpdateVariant::DropFuture => (self.drain_future(), 0),
            PreUpdateVariant::Nothing => (self.empty_drain(), 0),
        }
    }
}

#[cfg(test)]
mod test {
    /*
    use super::*;

    struct Logs(Vec<[DenseTransitionLog<char>; 2]>);

    impl Logs {
        fn new() -> Self {
            Self(vec![Default::default()])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            push: char,
            expected_transitions_len: usize,
            expected_pop: Option<char>,
        ) {
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = log1.push_and_pop_past(max_past_len, push);
                assert_eq!(
                    log1.transitions_len(),
                    expected_transitions_len,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );
                assert_eq!(
                    actual_pop, expected_pop,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );

                let before = log2.clone();
                let actual_drain: Vec<_> = log2.push_and_drain_past(max_past_len, push).collect();
                assert_eq!(
                    log2.transitions_len(),
                    expected_transitions_len,
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
                assert_eq!(
                    actual_drain.as_slice(),
                    expected_pop.as_slice(),
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
            }
        }
        fn forward_log(&mut self, expected_transition: Result<char, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transition = log.forward_log().cloned();
                assert_eq!(
                    actual_transition, expected_transition,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(&mut self, expected_transition: Result<char, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transition = log.backward_log().cloned();
                assert_eq!(
                    actual_transition, expected_transition,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn drain_future(&mut self, expected_future: Vec<char>, expected_transitions_len: usize) {
            self.0 = std::mem::take(&mut self.0)
                .into_iter()
                .flatten()
                .map(|mut log| {
                    let before = log.clone();
                    let actual_future: Vec<_> = log.drain_future().collect();
                    assert_eq!(
                        log.transitions_len(),
                        expected_transitions_len,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    assert_eq!(
                        actual_future, expected_future,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    [before, log]
                })
                .collect();
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut logs = Logs::new();
        logs.forward(2, 'a', 1, None);
        logs.forward(2, 'b', 2, None);
        // shortened log
        logs.forward(2, 'c', 2, Some('a'));

        logs.backward_log(Ok('c'));
        logs.backward_log(Ok('b'));
        // out of log, no mutations happend to the logs here
        logs.backward_log(Err(OutOfLog));

        logs.forward_log(Ok('b'));
        logs.forward_log(Ok('c'));
        // nothing ever logged past 'c', no mutations happend to the logs here
        logs.forward_log(Err(OutOfLog));

        logs.backward_log(Ok('c'));
        logs.backward_log(Ok('b'));

        logs.drain_future(vec!['b', 'c'], 0);

        // all entries are truncated as they are in the future
        logs.forward(2, 'd', 1, None);
    }
    */
}
