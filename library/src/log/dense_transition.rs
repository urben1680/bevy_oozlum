use core::fmt::Debug;
use std::collections::{
    TryReserveError, VecDeque,
    vec_deque::{Drain, Iter},
};

use bevy::reflect::{Reflect, std_traits::ReflectDefault};

use super::{INDEX_OOB, OutOfLog};

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DenseTransitionLog<T> {
    transitions: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::DenseTransitionLog;

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for DenseTransitionLog<T> {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<T>>, usize);
        type De = (WithCapacityWrapper<VecDeque<T>>, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.transitions), self.index)
        }
        fn from_with_capacity((WithCapacityWrapper(transitions), index): Self::De) -> Self {
            Self { transitions, index }
        }
    }

    impl<T> LoglessWithCapacity for DenseTransitionLog<T> {
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

impl<T> Default for DenseTransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> DenseTransitionLog<T> {
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
    pub fn push(&mut self, transition: T) {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        self.index = self.transitions.len();
    }
    pub fn push_and_pop_past(&mut self, max_past_len: usize, transition: T) -> Option<T> {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        if self.index >= max_past_len {
            self.transitions.pop_front()
        } else {
            self.index = self.transitions.len();
            None
        }
    }
    pub fn push_and_drain_past(&mut self, max_past_len: usize, transition: T) -> Drain<T> {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        let to_drain = self.transitions.len().saturating_sub(max_past_len);
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
        let to_drain = self.transitions.len().saturating_sub(max_past_len);
        self.index = self.transitions.len() - to_drain;
        self.transitions.range(..to_drain)
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> Drain<T> {
        self.transitions.drain(..to_drain)
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
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: DenseTransitionLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: DenseTransitionLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: DenseTransitionLog<char>,
        }

        let mut original = DenseTransitionLog::new();
        original.push('a');
        original.push('b');
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

        let test = |log: &DenseTransitionLog<char>, len, with_capacity| {
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
}
