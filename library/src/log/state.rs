use core::fmt::Debug;
use std::{
    collections::{TryReserveError, VecDeque},
    ops::Deref,
};

use bevy::{reflect::Reflect, utils::tracing::error};

use crate::{log::INDEX_OOB, meta::RevMeta};

use super::{LogIter, LoggedAt, OutOfLog};

#[derive(Debug, Default, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StateLog<T> {
    /// The log of states, with two partitions:
    /// - Past states in the indices `[0, self.index[`
    /// - Future states in the indices `[self.index, self.states.len()[`
    ///
    /// The present state is not part of this deque and traversing the log swaps
    /// the present state from before and now while keeping the above partitions.
    states: VecDeque<T>,
    /// The present state, easily accessible to read.
    present: T,
    /// The index of the nearest future state in `self.states`, if there is any.
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{
        LoglessState, LoglessWithCapacity, WithCapacity, WithCapacityWrapper,
    };

    use super::StateLog;

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessState for StateLog<T> {
        type Se<'se> = &'se T;
        type De = T;
        fn get_logless_state(&self) -> Self::Se<'_> {
            &self.present
        }
        fn from_logless_state(logless_state: Self::De) -> Self {
            logless_state.into()
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for StateLog<T> {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<T>>, &'se T, usize);
        type De = (WithCapacityWrapper<VecDeque<T>>, T, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.states), &self.present, self.index)
        }
        fn from_with_capacity(with_capacity: Self::De) -> Self {
            Self {
                states: with_capacity.0 .0,
                present: with_capacity.1,
                index: with_capacity.2,
            }
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessWithCapacity for StateLog<T> {
        type Se<'se> = (&'se T, usize);
        type De = (T, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (&self.present, self.states.capacity())
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self {
                states: VecDeque::with_capacity(logless_with_capacity.1),
                present: logless_with_capacity.0,
                index: 0,
            }
        }
    }
}

impl<T> From<T> for StateLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> Deref for StateLog<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.present
    }
}

impl<T> StateLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present,
            index: 0,
        }
    }
    pub fn with_capacity(present: T, capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(capacity),
            present,
            index: 0,
        }
    }
    pub fn into_inner(self) -> T {
        self.present
    }
    pub fn len(&self) -> usize {
        self.states.len()
    }
    pub fn capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn shrink_to_fit(&mut self) {
        self.states.shrink_to_fit()
    }
    pub fn unlogged_get_mut(&mut self) -> &mut T {
        &mut self.present
    }
    /// Most past state or `None` if the oldest state is considered to be the present state
    pub fn past_end(&self) -> Option<&T> {
        if self.index == 0 {
            return None;
        }
        self.states.front()
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        self.states.pop_front()
    }
    pub fn push_present(&mut self, state: T) {
        self.states.truncate(self.index);
        let before = core::mem::replace(&mut self.present, state);
        self.states.push_back(before);
        self.index += 1;
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.states.drain(self.index..)
    }
    pub fn clear(&mut self) {
        self.states.clear();
        self.index = 0;
    }
    pub fn clear_with(&mut self, present: T) {
        self.states.clear();
        self.present = present;
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        // from:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2
        // to:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1

        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        if let Some(now_future) = self.states.get_mut(index) {
            self.index = index;
            core::mem::swap(&mut self.present, now_future);
            return Ok(());
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct StateLogDebug {
            states_len: usize,
            index: usize,
        }

        let debug_struct = StateLogDebug {
            states_len: self.states.len(),
            index: self.index,
        };

        error!("{INDEX_OOB}, {debug_struct:#?}");
        Err(OutOfLog)
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        // from:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1
        // to:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2

        if let Some(now_future) = self.states.get_mut(self.index) {
            core::mem::swap(&mut self.present, now_future);
            self.index += 1;
            return Ok(());
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct StateLogDebug {
            states_len: usize,
            index: usize,
        }

        if self.index != self.states.len() {
            let debug_struct = StateLogDebug {
                states_len: self.states.len(),
                index: self.index,
            };
            error!("{INDEX_OOB}, {debug_struct:#?}");
        }

        Err(OutOfLog)
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        if self.index > max_past_len {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
        let excessive = self.index.saturating_sub(max_past_len);
        self.index -= excessive;
        self.states.drain(..excessive)
    }
}

impl<T: LoggedAt> StateLog<T> {
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<T> {
        let logged_at = self.past_end()?.logged_at();
        if !meta.contains(logged_at) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> impl LogIter<T> {
        // may be redundant but if not improves partition_point performance
        self.states.truncate(self.index);

        let ref_len = meta.past_world_states();
        let start = meta.oldest_world_state().0;
        let to = self
            .states
            .partition_point(|entry| !RevMeta::contains_buffered(start, entry, ref_len));
        self.index -= to;
        self.states.drain(..to)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::{
        log::{test::ForwardStrategy, PackedRevFrame},
        meta::RevMeta,
        RevFrame,
    };

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: StateLog<char>,
            #[serde(with = "crate::log::logless_state")]
            logless: StateLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: StateLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: StateLog<char>,
        }

        let mut log = StateLog::from('a');
        log.push_present('b');
        log.push_present('c');
        log.backward_log().expect("in log");

        let mut logs = Logs {
            full: log.clone(),
            logless: log.clone(),
            full_with_capacity: log.clone(),
            logless_with_capacity: log.clone(),
        };

        logs.full.reserve_exact(98);
        logs.logless.reserve_exact(98);
        logs.full_with_capacity.reserve_exact(98);
        logs.logless_with_capacity.reserve_exact(98);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            full,
            logless,
            full_with_capacity,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        assert_eq!(full.len(), 2, "serialized: {serialized}");
        assert_eq!(*full, 'b', "serialized: {serialized}");
        assert!(
            full.capacity() < 100,
            "actual capacity: {}, serialized: {serialized}",
            full.capacity()
        );

        assert_eq!(logless.len(), 0, "serialized: {serialized}");
        assert_eq!(*logless, 'b', "serialized: {serialized}");
        assert!(
            logless.capacity() < 100,
            "actual capacity: {}, serialized: {serialized}",
            logless.capacity()
        );

        assert_eq!(full_with_capacity.len(), 2, "serialized: {serialized}");
        assert_eq!(*full_with_capacity, 'b', "serialized: {serialized}");
        assert!(
            full_with_capacity.capacity() >= 100,
            "actual capacity: {}, serialized: {serialized}",
            full_with_capacity.capacity()
        );

        assert_eq!(logless_with_capacity.len(), 0, "serialized: {serialized}");
        assert_eq!(*logless_with_capacity, 'b', "serialized: {serialized}");
        assert!(
            logless_with_capacity.capacity() >= 100,
            "actual capacity: {}, serialized: {serialized}",
            logless_with_capacity.capacity()
        );
    }

    impl StateLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ForwardStrategy,
            push: u8,
            expected_log_len: usize,
            popped: Option<(u8, usize)>,
        ) {
            meta.queue_forward();
            meta.update();
            let before = self.clone();
            let push = (push, meta.present_world_state());
            self.push_present(push);
            let after_push = self.clone();
            let actual = match strategy {
                ForwardStrategy::PopPastByLen => self.pop_past_by_len(meta.past_world_states()),
                ForwardStrategy::PopPastByLoggedAt => self.pop_past_by_logged_at(meta),
                ForwardStrategy::DrainPastByLen | ForwardStrategy::DrainPastByLoggedAt => {
                    let mut actual: Vec<_> = match strategy {
                        ForwardStrategy::DrainPastByLen => {
                            self.drain_past_by_len(meta.past_world_states()).collect()
                        }
                        ForwardStrategy::DrainPastByLoggedAt => {
                            self.truncate_future_drain_past_by_logged_at(meta).collect()
                        }
                        _ => unreachable!(),
                    };
                    assert!(
                        actual.len() <= 1,
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}\npopped: {actual:#?}",                
                    );
                    actual.pop()
                }
            }.map(|(value, logged_at)| (value, logged_at.into()));
            assert_eq!(
                actual, popped,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.len(),
                expected_log_len,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                **self, push,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
        }
        fn test_forward_log(&mut self, meta: &mut RevMeta, state: Result<u8, u8>) {
            match state {
                Ok(state) => {
                    meta.queue_log(RevFrame(meta.present_world_state().0 + 1))
                        .unwrap();
                    meta.update();
                    let before = self.clone();
                    let result = self.forward_log();
                    assert_eq!(
                        result,
                        Ok(()),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_forward: {self:#?}",
                    );
                    assert_eq!(
                        **self,
                        (state, meta.present_world_state()),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_forward: {self:#?}",
                    );
                }
                Err(state) => {
                    let before = self.clone();
                    let result = self.forward_log();
                    assert_eq!(
                        result,
                        Err(OutOfLog),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_forward: {self:#?}",
                    );
                    assert_eq!(
                        **self,
                        (state, meta.present_world_state()),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_forward: {self:#?}",
                    );
                }
            }
        }
        fn test_backward_log(&mut self, meta: &mut RevMeta, state: Result<u8, u8>) {
            match state {
                Ok(state) => {
                    meta.queue_log(RevFrame(meta.present_world_state().0 - 1))
                        .unwrap();
                    meta.update();
                    let before = self.clone();
                    let result = self.backward_log();
                    assert_eq!(
                        result,
                        Ok(()),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_backward: {self:#?}",
                    );
                    assert_eq!(
                        **self,
                        (state, meta.present_world_state()),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_backward: {self:#?}",
                    );
                }
                Err(state) => {
                    let before = self.clone();
                    let result = self.backward_log();
                    assert_eq!(
                        result,
                        Err(OutOfLog),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_backward: {self:#?}",
                    );
                    assert_eq!(
                        **self,
                        (state, meta.present_world_state()),
                        "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_backward: {self:#?}",
                    );
                }
            }
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ForwardStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroUsize::new(3), 0, false);
            let mut log = StateLog::new((0, meta.present_world_state()));

            log.test_forward(meta, strategy, 1, 1, None);
            log.test_forward(meta, strategy, 2, 2, None);
            // shortened log
            log.test_forward(meta, strategy, 3, 2, Some((0, 0)));

            log.test_backward_log(meta, Ok(2));
            log.test_backward_log(meta, Ok(1));
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, Err(1));

            log.test_forward_log(meta, Ok(2));
            log.test_forward_log(meta, Ok(3));
            // nothing ever logged past 3, no mutations happend to both meta and log here
            log.test_forward_log(meta, Err(3));

            log.test_backward_log(meta, Ok(2));
            log.test_backward_log(meta, Ok(1));
            // all entries are truncated as they are in the future, the new logged entry increases len to 1
            log.test_forward(meta, strategy, 4, 1, None);
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<StateLog<PackedRevFrame>>();
    }
}
