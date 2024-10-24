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
    ///
    /// Never larger than `self.states.len()`
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
        fn from_logless_state(logless_state: Self::De) -> Result<Self, String> {
            Ok(logless_state.into())
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for StateLog<T> {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<T>>, &'se T, usize);
        type De = (WithCapacityWrapper<VecDeque<T>>, T, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.states), &self.present, self.index)
        }
        fn from_with_capacity(with_capacity: Self::De) -> Result<Self, String> {
            Ok(Self {
                states: with_capacity.0 .0,
                present: with_capacity.1,
                index: with_capacity.2,
            })
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessWithCapacity for StateLog<T> {
        type Se<'se> = (&'se T, usize);
        type De = (T, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (&self.present, self.states.capacity())
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Result<Self, String> {
            Ok(Self {
                states: VecDeque::with_capacity(logless_with_capacity.1),
                present: logless_with_capacity.0,
                index: 0,
            })
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
        // before:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2
        // after:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1

        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        if let Some(now_future) = self.states.get_mut(index) {
            self.index = index;
            core::mem::swap(&mut self.present, now_future);
            return Ok(());
        }
        error!(
            "{INDEX_OOB}T: {}, states.len(): {}, index: {}",
            std::any::type_name::<T>(),
            self.states.len(),
            self.index
        );
        Err(OutOfLog)
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        // before:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1
        // after:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2

        let now_future = self.states.get_mut(self.index).ok_or(OutOfLog)?;
        core::mem::swap(&mut self.present, now_future);
        self.index += 1;
        return Ok(());
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
        let start = meta.oldest_world_state();
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
        log::{
            test::{shorten_strategy, ShortenStrategy},
            PackedRevFrame,
        },
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

        let mut original = StateLog::from('a');
        original.push_present('b');
        original.push_present('c');
        original.backward_log().expect("in log");

        let mut logs = Logs {
            full: original.clone(),
            logless: original.clone(),
            full_with_capacity: original.clone(),
            logless_with_capacity: original.clone(),
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

        let test = |log: &StateLog<char>, len, with_capacity| {
            assert_eq!(
                **log, 'b',
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.len(),
                len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.capacity() >= 100,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.capacity()
            );
        };

        test(&full, 2, false);
        test(&logless, 0, false);
        test(&full_with_capacity, 2, true);
        test(&logless_with_capacity, 0, true);
    }

    impl StateLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            push: u8,
            expected_states_len: usize,
            expected_popped: Option<(u8, usize)>,
        ) {
            meta.queue_forward();
            meta.update();
            let before = self.clone();
            let push = (push, meta.present_world_state());
            self.push_present(push);
            let after_push = self.clone();
            let actual_popped = shorten_strategy!(self, meta, strategy, before, after_push);
            assert_eq!(
                actual_popped, expected_popped,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.len(),
                expected_states_len,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                **self, push,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
        }
        fn test_forward_log(&mut self, meta: &mut RevMeta, expected_state: u8, out_of_log: bool) {
            let before = self.clone();
            if out_of_log {
                let result = self.forward_log();
                assert_eq!(
                    result,
                    Err(OutOfLog),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            } else {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update();
                let result = self.forward_log();
                assert_eq!(
                    result,
                    Ok(()),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            }
            self.test_state(before, meta, expected_state);
        }
        fn test_backward_log(&mut self, meta: &mut RevMeta, expected_state: u8, out_of_log: bool) {
            let before = self.clone();
            if out_of_log {
                let result = self.backward_log();
                assert_eq!(
                    result,
                    Err(OutOfLog),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            } else {
                let frame = meta.present_world_state().wrapping_sub(1);
                meta.queue_log(frame).unwrap();
                meta.update();
                let result = self.backward_log();
                assert_eq!(
                    result,
                    Ok(()),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            }
            self.test_state(before, meta, expected_state);
        }
        fn test_state(&self, before: Self, meta: &RevMeta, state: u8) {
            assert_eq!(
                **self,
                (state, meta.present_world_state()),
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (u8, usize)>,
        ) -> Self {
            let before = self.clone();
            let mut clone = self.clone();
            let actual_future: Vec<_> = clone.drain_future().collect();
            let expected_future: Vec<_> = expected_future
                .into_iter()
                .map(|(state, frame)| (state, RevFrame(frame)))
                .collect();
            assert_eq!(
                actual_future, expected_future,
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroUsize::new(3), 0, false);
            let mut log = StateLog::new((0, meta.present_world_state()));

            log.test_forward(meta, strategy, 1, 1, None);
            log.test_forward(meta, strategy, 2, 2, None);
            // shortened log
            log.test_forward(meta, strategy, 3, 2, Some((0, 0)));

            log.test_backward_log(meta, 2, false);
            log.test_backward_log(meta, 1, false);
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, 1, true);

            log.test_forward_log(meta, 2, false);
            log.test_forward_log(meta, 3, false);
            // nothing ever logged past 3, no mutations happend to both meta and log here
            log.test_forward_log(meta, 3, true);

            log.test_backward_log(meta, 2, false);
            log.test_backward_log(meta, 1, false);

            let mut clone = log.test_drain_future([(2, 2), (3, 3)]);

            // all entries are truncated as they are in the future
            log.test_forward(meta, strategy, 4, 1, None);
            clone.test_forward(meta, strategy, 4, 1, None);
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<StateLog<PackedRevFrame>>();
    }
}
