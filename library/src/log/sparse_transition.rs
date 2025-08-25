use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque, vec_deque::Iter};

use super::{INDEX_OOB, OutOfLog, SparseDrain, SparseValue};

#[derive(Debug, Clone)]
pub struct SparseTransitionLog<T> {
    /// SparseValue.skips represents the number of None pushes before the transition in the struct.
    transitions: VecDeque<SparseValue<T>>,
    index: usize,
    /// For simplicity, this never gets reduced by `pop`/`drain_past_by_len`/`logged_at`.
    skips: usize,
    /// Used to check for OutOfLog error when calling `self.forward_log`/`logged_at`.
    ///
    /// For simplicity, this never gets reduced by `pop`/`drain_past_by_len`.
    skips_max: usize,
    past_len: usize,
}

#[cfg(feature = "serialize")]
mod serde_with {
    use serde::{Deserialize, Serialize};

    use crate::log::serialize::WithCapacity;

    use super::SparseTransitionLog;

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for SparseTransitionLog<T> {
        type Se<'se>
            = usize
        where
            T: 'se;
        type De = usize;
        fn get_with_capacity(&self) -> Self::Se<'_> {
            self.transitions.capacity()
        }
        fn from_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
    }
}

impl<T> Default for SparseTransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SparseTransitionLog<T> {
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            index: 0,
            skips: 0,
            skips_max: 0,
            past_len: 0,
        }
    }
    pub fn with_capacity(transitions_capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
            skips: 0,
            skips_max: 0,
            past_len: 0,
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
    pub fn drain_future(&mut self) -> SparseDrain<T> {
        self.skips_max = self.skips;
        SparseDrain(self.transitions.drain(self.index..))
    }
    pub fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
        self.past_len = 0;
        self.skips = 0;
        self.skips_max = 0;
    }
    fn push(&mut self, transition: Option<T>) {
        self.transitions.truncate(self.index);
        match transition {
            None => {
                self.skips += 1;
                self.past_len += 1;
            }
            Some(transition) => {
                self.transitions
                    .push_back(SparseValue::new(transition, self.skips));
                self.index += 1;
                self.skips = 0;
                self.past_len += 1;
            }
        }
        self.skips_max = self.skips;
    }
    pub fn push_and_pop_past(&mut self, max_past_len: usize, transition: Option<T>) -> Option<T> {
        self.push(transition);
        let excessive_len = self.past_len.checked_sub(max_past_len)?;
        let past_end = self.transitions.front()?;
        if excessive_len < past_end.len() {
            return None;
        }
        self.transitions.pop_front().map(|rare| {
            self.index -= 1;
            self.past_len -= rare.len();
            rare.value
        })
    }
    pub(super) fn push_and_iter_to_drain_past(
        &mut self,
        max_past_len: usize,
        transition: Option<T>,
    ) -> Iter<SparseValue<T>> {
        self.push(transition);
        let mut to_drain = 0;
        for entry in self.transitions.iter() {
            let less = self.past_len - entry.len();
            if less < max_past_len {
                break;
            }
            self.past_len = less;
            to_drain += 1;
        }
        self.index -= to_drain;
        self.transitions.range(..to_drain)
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> SparseDrain<T> {
        SparseDrain(self.transitions.drain(..to_drain))
    }
    pub fn push_and_drain_past(
        &mut self,
        max_past_len: usize,
        transition: Option<T>,
    ) -> SparseDrain<T> {
        self.push(transition);
        let mut to_drain = 0;
        for entry in self.transitions.iter() {
            let less = self.past_len - entry.len();
            if less < max_past_len {
                break;
            }
            self.past_len = less;
            to_drain += 1;
        }
        self.index -= to_drain;
        SparseDrain(self.transitions.drain(..to_drain))
    }
    pub fn backward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.past_len -= 1;
            Ok(None)
        } else {
            let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
            let entry = self.transitions.get_mut(index).expect(INDEX_OOB);
            self.index = index;
            self.skips = entry.skips();
            self.past_len -= 1;
            Ok(Some(&mut entry.value))
        }
    }
    pub fn forward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if let Some(entry) = self.transitions.get_mut(self.index) {
            self.past_len += 1;
            if self.skips < entry.skips() {
                self.skips += 1;
                Ok(None)
            } else {
                self.index += 1;
                self.skips = 0;
                Ok(Some(&mut entry.value))
            }
        } else if self.skips < self.skips_max {
            self.past_len += 1;
            self.skips += 1;
            Ok(None)
        } else {
            Err(OutOfLog)
        }
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
            #[serde(with = "crate::log::with_capacity")]
            logless_with_capacity: SparseTransitionLog<char>,
        }

        let mut original = SparseTransitionLog::new();
        original.push(Some('a'));
        original.push(Some('b'));
        original.backward_log().expect("in log");

        let mut logs = Logs {
            logless_with_capacity: original.clone(),
        };

        logs.logless_with_capacity.transitions_reserve_exact(98);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &SparseTransitionLog<char>, len, with_capacity| {
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

        test(&logless_with_capacity, 0, true);
    }

    struct Logs(Vec<[SparseTransitionLog<char>; 2]>);

    impl Logs {
        fn new() -> Self {
            Self(vec![Default::default()])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            push: Option<char>,
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
        fn forward_log(&mut self, expected_transition: Result<Option<char>, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transition = log.forward_log().map(|transition| transition.cloned());
                assert_eq!(
                    actual_transition, expected_transition,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(&mut self, expected_transition: Result<Option<char>, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transition: Result<Option<char>, OutOfLog> =
                    log.backward_log().map(|transition| transition.cloned());
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
                .collect()
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut logs = Logs::new();
        logs.forward(2, Some('a'), 1, None);
        logs.forward(2, None, 1, None);
        // shortened log
        logs.forward(2, Some('b'), 1, Some('a'));

        logs.backward_log(Ok(Some('b')));
        logs.backward_log(Ok(None));
        // out of log, no mutations happend to the logs here
        logs.backward_log(Err(OutOfLog));

        logs.forward_log(Ok(None));
        logs.forward_log(Ok(Some('b')));
        // nothing ever logged past 'b', no mutations happend to the logs here
        logs.forward_log(Err(OutOfLog));

        logs.backward_log(Ok(Some('b')));
        logs.backward_log(Ok(None));

        logs.drain_future(vec!['b'], 0);

        // all entries are truncated as they are in the future
        logs.forward(2, Some('c'), 1, None);
    }
}
