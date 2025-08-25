use std::{
    collections::{
        TryReserveError, VecDeque,
        vec_deque::{Drain, IterMut},
    },
    fmt::Debug,
};

use super::{
    EntryAmount, LogMut, OutOfLog, PushedTooMany, SparseDrain, SparseTransitionLog, USIZE_BYTES,
    ValueEntry,
};

#[derive(Debug, Clone)]
pub struct SparseTransitionsLog<T, U = (), const AMOUNT_BYTES: usize = USIZE_BYTES> {
    amounts: SparseTransitionLog<EntryAmount<U, AMOUNT_BYTES>>,
    transitions: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serialize")]
mod serde_with {
    use serde::{Deserialize, Serialize};

    use crate::log::serialize::WithCapacity;

    use super::SparseTransitionsLog;

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for SparseTransitionsLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se>
            = (usize, usize)
        where
            T: 'se,
            U: 'se;
        type De = (usize, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (self.entries_capacity(), self.transitions_capacity())
        }
        fn from_with_capacity((entries_capacity, transitions_capacity): Self::De) -> Self {
            Self::with_capacities(entries_capacity, transitions_capacity)
        }
    }
}

impl<T, U, const AMOUNT_BYTES: usize> Default for SparseTransitionsLog<T, U, AMOUNT_BYTES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U, const AMOUNT_BYTES: usize> SparseTransitionsLog<T, U, AMOUNT_BYTES> {
    pub const fn new() -> Self {
        Self {
            amounts: SparseTransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(entries_capacity: usize, transitions_capacity: usize) -> Self {
        Self {
            amounts: SparseTransitionLog::with_capacity(entries_capacity),
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
        }
    }
    pub fn entries_len(&self) -> usize {
        self.amounts.transitions_len()
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn entries_capacity(&self) -> usize {
        self.amounts.transitions_capacity()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn entries_is_empty(&self) -> bool {
        self.amounts.transitions_is_empty()
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn entries_reserve(&mut self, additional: usize) {
        self.amounts.transitions_reserve(additional)
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn entries_reserve_exact(&mut self, additional: usize) {
        self.amounts.transitions_reserve_exact(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn entries_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.transitions_try_reserve(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn entries_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.transitions_try_reserve_exact(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn entries_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.transitions_shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn entries_shrink_to_fit(&mut self) {
        self.amounts.transitions_shrink_to_fit()
    }
    pub fn transitions_shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }
    pub fn push_some_and_pop_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Option<ValueEntry<Drain<T>, U>> {
        self.try_push_some_and_pop_past(max_past_len, c)
            .unwrap_or_else(|err| panic!("{err}"))
    }
    pub fn try_push_some_and_pop_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<ValueEntry<Drain<T>, U>>, PushedTooMany<Drain<T>, U, AMOUNT_BYTES>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if AMOUNT_BYTES < USIZE_BYTES && pushed_amount != entry_amount.amount() {
            return Err(PushedTooMany {
                values: self.transitions.drain(self.index..),
                entry: entry_amount.entry,
            });
        }
        self.index = self.transitions.len();
        let pop = self
            .amounts
            .push_and_pop_past(max_past_len, Some(entry_amount))
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                self.index -= amount;
                ValueEntry {
                    value: self.transitions.drain(..amount),
                    entry: entry_amount.entry,
                }
            });
        Ok(pop)
    }
    pub fn push_none_and_pop_past(
        &mut self,
        max_past_len: usize,
    ) -> Option<ValueEntry<Drain<T>, U>> {
        self.transitions.truncate(self.index);
        self.amounts
            .push_and_pop_past(max_past_len, None)
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                self.index -= amount;
                ValueEntry {
                    value: self.transitions.drain(..amount),
                    entry: entry_amount.entry,
                }
            })
    }
    pub fn push_some_and_drain_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> (Drain<T>, SparseDrain<EntryAmount<U, AMOUNT_BYTES>>) {
        self.try_push_some_and_drain_past(max_past_len, c)
            .unwrap_or_else(|err| panic!("{err}"))
    }
    pub fn try_push_some_and_drain_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<
        (Drain<T>, SparseDrain<EntryAmount<U, AMOUNT_BYTES>>),
        PushedTooMany<Drain<T>, U, AMOUNT_BYTES>,
    > {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if AMOUNT_BYTES < USIZE_BYTES && pushed_amount != entry_amount.amount() {
            return Err(PushedTooMany {
                values: self.transitions.drain(self.index..),
                entry: entry_amount.entry,
            });
        }
        self.index = self.transitions.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, Some(entry_amount));
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain
            .map(|entry_amount| entry_amount.value.amount())
            .sum();
        self.index -= amount;
        Ok((
            self.transitions.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        ))
    }
    pub fn push_none_and_drain_past(
        &mut self,
        max_past_len: usize,
    ) -> (Drain<T>, SparseDrain<EntryAmount<U, AMOUNT_BYTES>>) {
        self.transitions.truncate(self.index);
        let to_drain = self.amounts.push_and_iter_to_drain_past(max_past_len, None);
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain
            .map(|entry_amount| entry_amount.value.amount())
            .sum();
        self.index -= amount;
        (
            self.transitions.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        )
    }
    pub fn drain_future(&mut self) -> (Drain<T>, SparseDrain<EntryAmount<U, AMOUNT_BYTES>>) {
        (
            self.transitions.drain(self.index..),
            self.amounts.drain_future(),
        )
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        self.transitions.clear();
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<Option<ValueEntry<IterMut<T>, &mut U>>, OutOfLog> {
        Ok(self.amounts.backward_log()?.map(|entry_amount| {
            let old_index = self.index;
            self.index -= entry_amount.amount();
            let value = self.transitions.range_mut(self.index..old_index);
            ValueEntry {
                value,
                entry: &mut entry_amount.entry,
            }
        }))
    }
    pub fn forward_log(&mut self) -> Result<Option<ValueEntry<IterMut<T>, &mut U>>, OutOfLog> {
        Ok(self.amounts.forward_log()?.map(|entry_amount| {
            let old_index = self.index;
            self.index += entry_amount.amount();
            let value = self.transitions.range_mut(old_index..self.index);
            ValueEntry {
                value,
                entry: &mut entry_amount.entry,
            }
        }))
    }
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::log::test::{collect_drain, collect_drain_result, collect_pop_result};

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            #[serde(with = "crate::log::with_capacity")]
            logless_with_capacity: SparseTransitionsLog<char, u8>,
        }

        let mut original = SparseTransitionsLog::new();
        original.push_some_and_pop_past(usize::MAX, |mut log| {
            log.extend(['a', 'b']);
            1
        });
        original.push_some_and_pop_past(usize::MAX, |mut log| {
            log.extend(['c', 'd']);
            2
        });
        original.backward_log().expect("in log");

        let mut logs = Logs {
            logless_with_capacity: original.clone(),
        };

        logs.logless_with_capacity.entries_reserve_exact(98);

        logs.logless_with_capacity.transitions_reserve_exact(196);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &SparseTransitionsLog<char, u8>,
                    entries_len,
                    transitions_len,
                    with_capacity| {
            assert_eq!(
                log.entries_len(),
                entries_len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.transitions_len(),
                transitions_len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.entries_capacity() >= 100,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.entries_capacity()
            );
            assert_eq!(
                log.transitions_capacity() >= 200,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.transitions_capacity()
            );
        };

        test(&logless_with_capacity, 0, 0, true);
    }

    struct Logs(Vec<[SparseTransitionsLog<char, char, 1>; 2]>);

    impl Logs {
        fn new() -> Self {
            Self(vec![Default::default()])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            push: Option<(Vec<char>, char)>,
            expected_transitions_len: usize,
            expected_entries_len: usize,
            expected_pop_or_amount_err: Result<Option<(Vec<char>, char)>, ()>,
        ) {
            let expected_pop_or_amount_err = expected_pop_or_amount_err.map_err(|()| push.clone());
            let expected_drained_or_amount_err = expected_pop_or_amount_err
                .clone()
                .map(|expected_drained| expected_drained.into_iter().collect::<Vec<_>>());
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = match push.clone() {
                    Some((transitions, entry)) => {
                        log1.try_push_some_and_pop_past(max_past_len, |mut log| {
                            log.extend(transitions);
                            entry
                        })
                    }
                    None => Ok(log1.push_none_and_pop_past(max_past_len)),
                };
                let actual_pop = collect_pop_result(actual_pop).map_err(|err| Some(err));
                assert_eq!(
                    actual_pop,
                    expected_pop_or_amount_err.clone(),
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );
                assert_eq!(
                    log1.transitions_len(),
                    expected_transitions_len,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );
                assert_eq!(
                    log1.entries_len(),
                    expected_entries_len,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );

                let before = log2.clone();
                let actual_drain = match push.clone() {
                    Some((transitions, entry)) => {
                        log2.try_push_some_and_drain_past(max_past_len, |mut log| {
                            log.extend(transitions);
                            entry
                        })
                    }
                    None => Ok(log2.push_none_and_drain_past(max_past_len)),
                };
                let actual_drain = collect_drain_result(actual_drain).map_err(|err| Some(err));
                assert_eq!(
                    actual_drain,
                    expected_drained_or_amount_err.clone(),
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
                assert_eq!(
                    log2.transitions_len(),
                    expected_transitions_len,
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
                assert_eq!(
                    log2.entries_len(),
                    expected_entries_len,
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
            }
        }
        fn forward_log(
            &mut self,
            expected_transitions: Result<Option<(Vec<char>, char)>, OutOfLog>,
        ) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transitions = log.forward_log().map(|value_entry| {
                    value_entry.map(|value_entry| {
                        (
                            value_entry.value.map(|state| *state).collect::<Vec<_>>(),
                            *value_entry.entry,
                        )
                    })
                });
                assert_eq!(
                    actual_transitions, expected_transitions,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(
            &mut self,
            expected_transitions: Result<Option<(Vec<char>, char)>, OutOfLog>,
        ) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transitions = log.backward_log().map(|value_entry| {
                    value_entry.map(|value_entry| {
                        (
                            value_entry.value.map(|state| *state).collect::<Vec<_>>(),
                            *value_entry.entry,
                        )
                    })
                });
                assert_eq!(
                    actual_transitions, expected_transitions,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn drain_future(
            &mut self,
            expected_future: Vec<(Vec<char>, char)>,
            expected_transitions_len: usize,
            expected_entries_len: usize,
        ) {
            self.0 = std::mem::take(&mut self.0)
                .into_iter()
                .flatten()
                .map(|mut log| {
                    let before = log.clone();
                    let actual_future = collect_drain(log.drain_future());
                    assert_eq!(
                        log.transitions_len(),
                        expected_transitions_len,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    assert_eq!(
                        log.entries_len(),
                        expected_entries_len,
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
        logs.forward(2, Some((vec!['a'; 2], 'A')), 2, 1, Ok(None));
        logs.forward(2, None, 2, 1, Ok(None));
        // shortened log
        logs.forward(
            2,
            Some((vec!['b'; 3], 'B')),
            3,
            1,
            Ok(Some((vec!['a'; 2], 'A'))),
        );

        logs.backward_log(Ok(Some((vec!['b'; 3], 'B'))));
        logs.backward_log(Ok(None));
        // out of log, no mutations happend to the logs here
        logs.backward_log(Err(OutOfLog));

        logs.forward_log(Ok(None));
        logs.forward_log(Ok(Some((vec!['b'; 3], 'B'))));
        // nothing ever logged past 'B', no mutations happend to the logs here
        logs.forward_log(Err(OutOfLog));

        logs.backward_log(Ok(Some((vec!['b'; 3], 'B'))));
        logs.backward_log(Ok(None));

        logs.drain_future(vec![(vec!['b'; 3], 'B')], 0, 0);

        // all entries are truncated as they are in the future
        logs.forward(2, Some((vec!['c'; 4], 'C')), 4, 1, Ok(None));

        // storing too many transitions fails
        logs.forward(2, Some((vec!['d'; 256], 'D')), 4, 1, Err(()));
    }
}
