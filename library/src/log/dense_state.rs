use core::fmt::Debug;
use std::{
    collections::{
        TryReserveError, VecDeque,
        vec_deque::{Drain, Iter},
    },
    fmt::Display,
    hash::Hash,
    ops::Deref,
};

use bevy::reflect::Reflect;

use super::{INDEX_OOB, OutOfLog};

#[derive(Debug, Default, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DenseStateLog<T> {
    /// The log of states, with two partitions:
    /// - Past states in `..self.index`
    /// - Future states in `self.index..`
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

impl<T: Display> Display for DenseStateLog<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.present.fmt(f)
    }
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{
        LoglessState, LoglessWithCapacity, WithCapacity, WithCapacityWrapper,
    };

    use super::DenseStateLog;

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessState for DenseStateLog<T> {
        type Se<'se> = &'se T;
        type De = T;
        fn get_logless_state(&self) -> Self::Se<'_> {
            &self.present
        }
        fn from_logless_state(logless_state: Self::De) -> Self {
            logless_state.into()
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for DenseStateLog<T> {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<T>>, &'se T, usize);
        type De = (WithCapacityWrapper<VecDeque<T>>, T, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.states), &self.present, self.index)
        }
        fn from_with_capacity((WithCapacityWrapper(states), present, index): Self::De) -> Self {
            Self {
                states,
                present,
                index,
            }
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessWithCapacity for DenseStateLog<T> {
        type Se<'se> = (&'se T, usize);
        type De = (T, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (&self.present, self.states.capacity())
        }
        fn from_logless_with_capacity((present, capacity): Self::De) -> Self {
            Self {
                states: VecDeque::with_capacity(capacity),
                present,
                index: 0,
            }
        }
    }
}

impl<T> From<T> for DenseStateLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> Deref for DenseStateLog<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.present
    }
}

impl<T: PartialEq> PartialEq<Self> for DenseStateLog<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: PartialEq> PartialEq<T> for DenseStateLog<T> {
    fn eq(&self, other: &T) -> bool {
        **self == *other
    }
}

impl<T: PartialOrd> PartialOrd<Self> for DenseStateLog<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: PartialOrd> PartialOrd<T> for DenseStateLog<T> {
    fn partial_cmp(&self, other: &T) -> Option<std::cmp::Ordering> {
        (**self).partial_cmp(other)
    }
}

impl<T: Hash> Hash for DenseStateLog<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T> DenseStateLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present,
            index: 0,
        }
    }
    pub fn with_capacity(present: T, states_capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(states_capacity),
            present,
            index: 0,
        }
    }
    pub fn into_inner(self) -> T {
        self.present
    }
    pub fn states_len(&self) -> usize {
        self.states.len()
    }
    pub fn states_capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn states_is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn states_reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn states_shrink_to_fit(&mut self) {
        self.states.shrink_to_fit()
    }
    pub fn push_and_pop_past(&mut self, max_past_len: usize, state: T) -> Option<T> {
        self.states.truncate(self.index);
        let before = core::mem::replace(&mut self.present, state);
        self.states.push_back(before);
        if self.index >= max_past_len {
            self.states.pop_front()
        } else {
            self.index += 1;
            None
        }
    }
    pub fn push_and_drain_past(&mut self, max_past_len: usize, state: T) -> Drain<T> {
        self.states.truncate(self.index);
        let before = core::mem::replace(&mut self.present, state);
        self.states.push_back(before);
        self.index += 1;
        let to_drain = self.index.saturating_sub(max_past_len);
        self.index -= to_drain;
        self.states.drain(..to_drain)
    }
    pub(super) fn push_and_iter_to_drain_past(&mut self, max_past_len: usize, state: T) -> Iter<T> {
        self.states.truncate(self.index);
        let before = core::mem::replace(&mut self.present, state);
        self.states.push_back(before);
        self.index += 1;
        let to_drain = self.index.saturating_sub(max_past_len);
        self.index -= to_drain;
        self.states.range(..to_drain)
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> Drain<T> {
        self.states.drain(..to_drain)
    }
    pub fn drain_future(&mut self) -> Drain<T> {
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
        let now_future = self.states.get_mut(index).expect(INDEX_OOB);
        self.index = index;
        core::mem::swap(&mut self.present, now_future);
        Ok(())
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
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: DenseStateLog<char>,
            #[serde(with = "crate::log::logless_state")]
            logless: DenseStateLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: DenseStateLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: DenseStateLog<char>,
        }

        let mut original = DenseStateLog::from('a');
        original.push_and_pop_past(usize::MAX, 'b');
        original.push_and_pop_past(usize::MAX, 'c');
        original.backward_log().expect("in log");

        let mut logs = Logs {
            full: original.clone(),
            logless: original.clone(),
            full_with_capacity: original.clone(),
            logless_with_capacity: original.clone(),
        };

        logs.full.states_reserve_exact(98);
        logs.logless.states_reserve_exact(98);
        logs.full_with_capacity.states_reserve_exact(98);
        logs.logless_with_capacity.states_reserve_exact(98);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            full,
            logless,
            full_with_capacity,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &DenseStateLog<char>, len, with_capacity| {
            assert_eq!(
                **log, 'b',
                "\nbefore: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.states_len(),
                len,
                "\nbefore: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.states_capacity() >= 100,
                with_capacity,
                "\nbefore: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.states_capacity()
            );
        };

        test(&full, 2, false);
        test(&logless, 0, false);
        test(&full_with_capacity, 2, true);
        test(&logless_with_capacity, 0, true);
    }

    #[test]
    fn clear() {
        let mut original = DenseStateLog::new(1);
        original.push_and_pop_past(usize::MAX, 2);
        original.push_and_pop_past(usize::MAX, 3);
        original.backward_log().expect("in log");

        let mut log = original.clone();
        log.clear();
        assert_eq!(*log, 2, "\nlog: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "\nlog: {log:#?}\noriginal: {original:#?}"
        );

        let mut log = original.clone();
        log.clear_with(4);
        assert_eq!(*log, 4, "\nlog: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "\nlog: {log:#?}\noriginal: {original:#?}"
        );
    }

    struct Logs(Vec<[DenseStateLog<char>; 2]>);

    impl Logs {
        fn new(state: char) -> Self {
            Self(vec![[state.into(), state.into()]])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            push: char,
            expected_states_len: usize,
            expected_pop: Option<char>,
        ) {
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = log1.push_and_pop_past(max_past_len, push);
                assert_eq!(**log1, push, "\nbefore: {before:#?}\nafter: {log1:#?}");
                assert_eq!(
                    actual_pop, expected_pop,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );
                assert_eq!(
                    log1.states_len(),
                    expected_states_len,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );

                let before = log2.clone();
                let actual_drain: Vec<_> = log2.push_and_drain_past(max_past_len, push).collect();
                assert_eq!(**log2, push, "\nbefore: {before:#?}\nafter: {log2:#?}");
                assert_eq!(
                    actual_drain.as_slice(),
                    expected_pop.as_slice(),
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
                assert_eq!(
                    log2.states_len(),
                    expected_states_len,
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
            }
        }
        fn forward_log(&mut self, expected_state: char, expected_out_of_log: bool) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_out_of_log = log.forward_log().is_err();
                assert_eq!(
                    actual_out_of_log, expected_out_of_log,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
                assert_eq!(
                    **log, expected_state,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(&mut self, expected_state: char, expected_out_of_log: bool) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_out_of_log = log.backward_log().is_err();
                assert_eq!(
                    actual_out_of_log, expected_out_of_log,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
                assert_eq!(
                    **log, expected_state,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn drain_future(&mut self, expected_future: Vec<char>, expected_states_len: usize) {
            self.0 = std::mem::take(&mut self.0)
                .into_iter()
                .flatten()
                .map(|mut log| {
                    let before = log.clone();
                    let actual_future: Vec<_> = log.drain_future().collect();
                    assert_eq!(
                        actual_future, expected_future,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    assert_eq!(
                        log.states_len(),
                        expected_states_len,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    [before, log]
                })
                .collect();
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut logs = Logs::new('a');
        logs.forward(2, 'b', 1, None);
        logs.forward(2, 'c', 2, None);
        // shortened log
        logs.forward(2, 'd', 2, Some('a'));

        logs.backward_log('c', false);
        logs.backward_log('b', false);
        // out of log, no mutations happend to the logs here
        logs.backward_log('b', true);

        logs.forward_log('c', false);
        logs.forward_log('d', false);
        // nothing ever logged past 'd', no mutations happend to the logs here
        logs.forward_log('d', true);

        logs.backward_log('c', false);
        logs.backward_log('b', false);

        logs.drain_future(vec!['c', 'd'], 0);

        // all entries are truncated as they are in the future
        logs.forward(2, 'e', 1, None);
    }
}
