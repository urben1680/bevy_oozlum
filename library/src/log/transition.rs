use core::fmt::Debug;
use std::collections::{VecDeque, vec_deque::Iter};

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
    pub(super) fn drain_future(&mut self) -> TransitionDrains<T> {
        TransitionDrains {
            transitions: self.transitions.drain(self.index..),
            past_len: 0,
        }
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> TransitionDrain<T> {
        self.transitions.drain(..to_drain)
    }
    pub(super) fn truncate_future(&mut self) {
        self.transitions.truncate(self.index);
    }
    pub(super) fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
    }
    pub(super) fn empty_drain(&mut self) -> TransitionDrains<T> {
        TransitionDrains {
            transitions: self.transitions.drain(..0),
            past_len: 0,
        }
    }
    pub(super) fn full_drain(&mut self) -> TransitionDrains<T> {
        let past_len = self.index;
        self.index = 0;
        TransitionDrains {
            transitions: self.transitions.drain(..),
            past_len,
        }
    }
    // todo: mention in docs that OutOfLog is not reliable but instead that the log is in-log
    // when it should be
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
