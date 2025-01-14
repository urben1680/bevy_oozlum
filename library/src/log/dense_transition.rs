use core::fmt::Debug;
use std::collections::{vec_deque::Drain, TryReserveError, VecDeque};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

use crate::meta::RevMeta;

use super::{index_oob, partition_point, LoggedAt, OutOfLog};

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionLog<T> {
    transitions: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::TransitionLog;

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for TransitionLog<T> {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<T>>, usize);
        type De = (WithCapacityWrapper<VecDeque<T>>, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.transitions), self.index)
        }
        fn from_with_capacity((WithCapacityWrapper(transitions), index): Self::De) -> Self {
            Self { transitions, index }
        }
    }

    impl<T> LoglessWithCapacity for TransitionLog<T> {
        type Se<'se>
            = usize
        where
            T: 'se;
        type De = usize;
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            self.transitions.capacity()
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
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
        }
    }
    pub fn with_capacity(transitions_capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
        }
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }
    pub fn push_present(&mut self, transition: T) {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        self.index = self.transitions.len();
    }
    pub fn drain_future(&mut self) -> Drain<T> {
        self.transitions.drain(self.index..)
    }
    pub fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<&mut T, OutOfLog> {
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        let transition = self.transitions.get_mut(index).ok_or_else(index_oob)?;
        self.index = index;
        Ok(transition)
    }
    pub fn forward_log(&mut self) -> Result<&mut T, OutOfLog> {
        self.transitions
            .get_mut(self.index)
            .inspect(|_| self.index += 1)
            .ok_or(OutOfLog)
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        if self.index == 0 {
            // if the current log position is at the past end, transitions.front() is not a past value but a future value
            return None;
        }
        if self.index > max_past_len {
            self.index -= 1;
            self.transitions.pop_front()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> Drain<T> {
        let excessive = self.index.saturating_sub(max_past_len);
        self.index -= excessive;
        self.transitions.drain(..excessive)
    }
}

impl<T: LoggedAt> TransitionLog<T> {
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<T> {
        if self.index == 0 {
            // if the current log position is at the past end, transitions.front() is not a past value but a future value
            return None;
        }
        let logged_at = self.transitions.front()?.logged_at();
        if !meta.past_contains(logged_at) {
            self.index -= 1;
            self.transitions.pop_front()
        } else {
            None
        }
    }
    pub fn drain_past_by_logged_at(&mut self, meta: &RevMeta) -> Drain<T> {
        let to = partition_point(&self.transitions, self.index, meta);
        self.index -= to;
        self.transitions.drain(..to)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU32;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::{
        frame::RevFrame,
        log::test::{shorten_strategy, ShortenStrategy},
        meta::RevMeta,
    };

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: TransitionLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: TransitionLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: TransitionLog<char>,
        }

        let mut original = TransitionLog::new();
        original.push_present('a');
        original.push_present('b');
        original.backward_log().expect("in log");

        let mut logs = Logs {
            full: original.clone(),
            full_with_capacity: original.clone(),
            logless_with_capacity: original.clone(),
        };

        logs.full.transitions_reserve_exact(98);
        logs.full_with_capacity.transitions_reserve_exact(98);
        logs.logless_with_capacity.transitions_reserve_exact(98);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            full,
            full_with_capacity,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &TransitionLog<char>, len, with_capacity| {
            assert_eq!(
                log.transitions_len(),
                len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.transitions_capacity() >= 100,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.transitions_capacity()
            );
        };

        test(&full, 2, false);
        test(&full_with_capacity, 2, true);
        test(&logless_with_capacity, 0, true);
    }

    impl TransitionLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            push: u8,
            expected_transitions_len: usize,
            expected_popped: Option<(u8, u32)>,
        ) {
            meta.queue_forward();
            meta.update(|_, _| {});
            let before = self.clone();
            let push = (push, meta.present_world_state());
            self.push_present(push);
            let after_push = self.clone();
            let actual_popped = shorten_strategy!(
                self,
                meta,
                strategy,
                meta.past_world_states(),
                before,
                after_push
            );
            assert_eq!(
                actual_popped, expected_popped,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.transitions_len(),
                expected_transitions_len,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transition: Result<u8, OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transition = expected_transition.map(|transition| {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
                (transition, frame)
            });
            let actual_transition = self.forward_log().map(|transition| *transition);
            assert_eq!(
                actual_transition, expected_transition,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            )
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transition: Result<u8, OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transition = expected_transition.map(|transition| {
                let frame = meta.present_world_state();
                meta.queue_log(frame.wrapping_sub(1)).unwrap();
                meta.update(|_, _| {});
                (transition, frame)
            });
            let actual_transition = self.backward_log().map(|transition| *transition);
            assert_eq!(
                actual_transition, expected_transition,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            )
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (u8, u32)>,
            expected_transitions_len: usize,
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
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            assert_eq!(
                clone.transitions_len(),
                expected_transitions_len,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut log = TransitionLog::new();

            log.test_forward(meta, strategy, 1, 1, None);
            log.test_forward(meta, strategy, 2, 2, None);
            // shortened log
            log.test_forward(meta, strategy, 3, 2, Some((1, 1)));

            log.test_backward_log(meta, Ok(3));
            log.test_backward_log(meta, Ok(2));
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, Err(OutOfLog));

            log.test_forward_log(meta, Ok(2));
            log.test_forward_log(meta, Ok(3));
            // out of log, no mutations happend to both meta and log here
            log.test_forward_log(meta, Err(OutOfLog));

            log.test_backward_log(meta, Ok(3));
            log.test_backward_log(meta, Ok(2));

            let clone = log.test_drain_future([(2, 2), (3, 3)], 0);

            for mut log in [log, clone] {
                // all entries are truncated as they are in the future
                log.test_forward(meta, strategy, 4, 1, None);
            }
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<TransitionLog<RevFrame>>();
    }
}
