use core::fmt::Debug;
use std::{
    collections::{TryReserveError, VecDeque, vec_deque::Iter},
    fmt::Display,
    hash::Hash,
    ops::Deref,
};

use bevy::reflect::Reflect;

use super::{INDEX_OOB, OutOfLog, SparseDrain, SparseValue};

#[derive(Debug, Default, Clone, Reflect)]
pub struct SparseStateLog<T> {
    /// SparseValue.skips represents the number of None pushes after the state in the struct
    states: VecDeque<SparseValue<T>>,
    present: T,
    index: usize,
    skips: usize,
    skips_max: usize,
    past_len: usize,
}

impl<T: Display> Display for SparseStateLog<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.present.fmt(f)
    }
}

#[cfg(feature = "serialize")]
mod serde_with {
    use serde::{Deserialize, Serialize};

    use crate::log::serialize::WithCapacity;

    use super::SparseStateLog;

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> Serialize for SparseStateLog<T> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            self.present.serialize(serializer)
        }
    }

    impl<'de, T: Serialize + Deserialize<'de> + 'static> Deserialize<'de> for SparseStateLog<T> {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            T::deserialize(deserializer).map(Into::into)
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for SparseStateLog<T> {
        type Se<'se> = (&'se T, usize);
        type De = (T, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (&self.present, self.states_capacity())
        }
        fn from_with_capacity((present, states_capacity): Self::De) -> Self {
            Self::with_capacity(present, states_capacity)
        }
    }
}

impl<T> From<T> for SparseStateLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> Deref for SparseStateLog<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.present
    }
}

impl<T: PartialEq> PartialEq<Self> for SparseStateLog<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: PartialEq> PartialEq<T> for SparseStateLog<T> {
    fn eq(&self, other: &T) -> bool {
        **self == *other
    }
}

impl<T: PartialOrd> PartialOrd<Self> for SparseStateLog<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: PartialOrd> PartialOrd<T> for SparseStateLog<T> {
    fn partial_cmp(&self, other: &T) -> Option<std::cmp::Ordering> {
        (**self).partial_cmp(other)
    }
}

impl<T: Hash> Hash for SparseStateLog<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

impl<T> SparseStateLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present,
            index: 0,
            skips: 0,
            skips_max: 0,
            past_len: 0,
        }
    }
    pub fn with_capacity(present: T, states_capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(states_capacity),
            ..Self::new(present)
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
    pub fn drain_future(&mut self) -> SparseDrain<T> {
        SparseDrain(self.states.drain(self.index..))
    }
    pub fn clear(&mut self) {
        self.states.clear();
        self.index = 0;
        self.skips = 0;
        self.skips_max = 0;
        self.past_len = 0;
    }
    pub fn clear_with(&mut self, present: T) {
        self.present = present;
        self.clear();
    }
    fn push(&mut self, state: Option<T>) {
        self.states.truncate(self.index);
        match state {
            None => {
                self.skips += 1;
                self.past_len += 1;
            }
            Some(state) => {
                let previous = core::mem::replace(&mut self.present, state);
                self.states
                    .push_back(SparseValue::new(previous, self.skips));
                self.skips = 0;
                self.index += 1;
                self.past_len += 1;
            }
        }
        self.skips_max = self.skips;
    }
    // todo push_if_neq api
    pub fn push_and_pop_past(&mut self, max_past_len: usize, state: Option<T>) -> Option<T> {
        self.push(state);
        let excessive_len = self
            .past_len
            .checked_sub(max_past_len)
            .filter(|len| *len > 0)?;
        let past_end = self.states.front()?;
        if excessive_len < past_end.len() {
            return None;
        }
        self.states.pop_front().map(|sparse_value| {
            self.index -= 1;
            self.past_len -= sparse_value.len();
            sparse_value.value
        })
    }
    pub fn push_and_drain_past(&mut self, max_past_len: usize, state: Option<T>) -> SparseDrain<T> {
        self.push(state);
        let mut to_drain = 0;
        for entry in self.states.iter() {
            let less = self.past_len - entry.len();
            if less < max_past_len {
                break;
            }
            self.past_len = less;
            to_drain += 1;
        }
        self.index -= to_drain;
        SparseDrain(self.states.drain(..to_drain))
    }
    pub(super) fn push_and_iter_to_drain_past(
        &mut self,
        max_past_len: usize,
        state: Option<T>,
    ) -> Iter<SparseValue<T>> {
        self.push(state);
        let mut to_drain = 0;
        for entry in self.states.iter() {
            let less = self.past_len - entry.len();
            if less < max_past_len {
                break;
            }
            self.past_len = less;
            to_drain += 1;
        }
        self.index -= to_drain;
        self.states.range(..to_drain)
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> SparseDrain<T> {
        SparseDrain(self.states.drain(..to_drain))
    }
    pub fn backward_log(&mut self) -> Result<bool, OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.past_len -= 1;
            return Ok(false);
        }
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        if !self.swap_state_and_skips_max(index) {
            panic!("{INDEX_OOB}");
        }
        self.index = index;
        self.skips = self.skips_max;
        self.past_len -= 1;
        Ok(true)
    }
    pub fn forward_log(&mut self) -> Result<bool, OutOfLog> {
        if self.skips < self.skips_max {
            self.past_len += 1;
            self.skips += 1;
            Ok(false)
        } else if self.swap_state_and_skips_max(self.index) {
            self.past_len += 1;
            self.index += 1;
            self.skips = 0;
            Ok(true)
        } else {
            Err(OutOfLog)
        }
    }
    fn swap_state_and_skips_max(&mut self, index: usize) -> bool {
        self.states
            .get_mut(index)
            .map(|entry| {
                let mut skips_max = self.skips_max.to_ne_bytes();
                core::mem::swap(&mut self.present, &mut entry.value);
                core::mem::swap(&mut skips_max, &mut entry.skips_ne);
                self.skips_max = usize::from_ne_bytes(skips_max);
            })
            .is_some()
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
            logless: SparseStateLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            logless_with_capacity: SparseStateLog<char>,
        }

        let mut original = SparseStateLog::from('a');
        original.push(Some('b'));
        original.push(Some('c'));
        original.backward_log().expect("in log");

        let mut logs = Logs {
            logless: original.clone(),
            logless_with_capacity: original.clone(),
        };

        logs.logless.states_reserve_exact(98);
        logs.logless_with_capacity.states_reserve_exact(98);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            logless,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &SparseStateLog<char>, len, with_capacity| {
            assert_eq!(
                **log, 'b',
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.states_len(),
                len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.states_capacity() >= 100,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.states_capacity()
            );
        };

        test(&logless, 0, false);
        test(&logless_with_capacity, 0, true);
    }

    #[test]
    fn clear() {
        let mut original = SparseStateLog::new(1);
        original.push(Some(2));
        original.push(None);
        original.push(Some(3));
        original.backward_log().expect("in log");

        let mut log = original.clone();
        log.clear();
        assert_eq!(*log, 2, "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );

        let mut log = original.clone();
        log.clear_with(4);
        assert_eq!(*log, 4, "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );
    }

    struct Logs(Vec<[SparseStateLog<char>; 2]>);

    impl Logs {
        fn new(state: char) -> Self {
            Self(vec![[state.into(), state.into()]])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            state: char,
            push: bool,
            expected_states_len: usize,
            expected_pop: Option<char>,
        ) {
            let push = push.then_some(state);
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = log1.push_and_pop_past(max_past_len, push);
                assert_eq!(**log1, state, "\nbefore: {before:#?}\nafter: {log1:#?}");
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
                assert_eq!(**log2, state, "\nbefore: {before:#?}\nafter: {log2:#?}");
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
        fn forward_log(&mut self, expected_state: char, expected_result: Result<bool, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_result = log.forward_log();
                assert_eq!(
                    actual_result, expected_result,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
                assert_eq!(
                    **log, expected_state,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(&mut self, expected_state: char, expected_result: Result<bool, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_result = log.backward_log();
                assert_eq!(
                    actual_result, expected_result,
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
        logs.forward(2, 'a', false, 0, None);
        logs.forward(2, 'b', true, 1, None);
        // does not pop yet because the skip after the initial state is still in log range
        logs.forward(2, 'c', true, 2, None);
        // shortened log
        logs.forward(2, 'c', false, 1, Some('a'));

        logs.backward_log('c', Ok(false));
        logs.backward_log('b', Ok(true));
        // out of log, no mutations happend to the logs here
        logs.backward_log('b', Err(OutOfLog));

        logs.forward_log('c', Ok(true));
        logs.forward_log('c', Ok(false));
        // nothing ever logged past 'c', no mutations happend to the logs here
        logs.forward_log('c', Err(OutOfLog));

        logs.backward_log('c', Ok(false));
        logs.backward_log('b', Ok(true));

        logs.drain_future(vec!['c'], 0);

        // all entries are truncated as they are in the future
        logs.forward(2, 'b', false, 0, None);
    }
}
