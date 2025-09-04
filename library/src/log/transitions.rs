use std::{
    collections::{
        VecDeque,
        vec_deque::{Drain, IterMut},
    },
    fmt::Debug,
};

use crate::{log::PreUpdateVariant, meta::RevMeta};

use super::{OutOfLog, TransitionLog};

#[derive(Debug)]
pub struct TransitionsLog<T, U = ()> {
    amounts: TransitionLog<EntryAmount<U>>,
    transitions: VecDeque<T>,
    index: usize,
}

impl<T, U> Default for TransitionsLog<T, U> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U> TransitionsLog<T, U> {
    pub const fn new() -> Self {
        Self {
            amounts: TransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn push_and_truncate_past<IntoU: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> IntoU,
    ) {
        let (transition_drain, amount_drain) =
            self.push_and_get_transition_and_amount_drain(max_past_len, c);
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..transition_drain);
        self.amounts.drain_past(amount_drain);
    }
    pub fn push_and_drain_past<IntoU: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> IntoU,
    ) -> (Drain<T>, Drain<EntryAmount<U>>) {
        // for the + 1, see the comment in TransitionLog::push_and_drain_past
        let (transition_drain, amount_drain) =
            self.push_and_get_transition_and_amount_drain(max_past_len + 1, c);
        (
            self.transitions.drain(..transition_drain),
            self.amounts.drain_past(amount_drain),
        )
    }
    fn push_and_get_transition_and_amount_drain<IntoU: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> IntoU,
    ) -> (usize, usize) {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount {
            entry,
            amount: pushed_amount,
        };
        self.index = self.transitions.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, entry_amount);
        let amount_drain = to_drain.len();
        let transition_drain: usize = to_drain.map(|entry_amount| entry_amount.amount).sum();
        self.index -= transition_drain;
        (transition_drain, amount_drain)
    }
    fn truncate_future(&mut self) {
        self.transitions.truncate(self.index);
        self.amounts.truncate_future();
    }
    fn truncate_past(&mut self) {
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..self.index);
        self.index = 0;
        self.amounts.truncate_past();
    }
    fn drain_future(&mut self) -> (Drain<T>, Drain<EntryAmount<U>>) {
        (
            self.transitions.drain(self.index..),
            self.amounts.drain_future(),
        )
    }
    fn clear(&mut self) {
        self.transitions.clear();
        self.amounts.clear();
        self.index = 0;
    }
    fn empty_drain(&mut self) -> (Drain<T>, Drain<EntryAmount<U>>) {
        (self.transitions.drain(..0), self.amounts.empty_drain())
    }
    pub fn backward_log(&mut self) -> Result<ValueEntry<IterMut<T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.backward_log()?;
        self.index -= entry_amount.amount;
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
    }
    pub fn forward_log(&mut self) -> Result<ValueEntry<IterMut<T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.forward_log()?;
        self.index += entry_amount.amount;
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
    }
    pub fn pre_update(&mut self, meta: &RevMeta) {
        match self.amounts.pre_update_check(meta) {
            PreUpdateVariant::DropLog => self.clear(),
            PreUpdateVariant::DropFuture => self.truncate_future(),
            PreUpdateVariant::Nothing => {}
        }
    }
    pub fn pre_update_drain_past<'a, 'm>(
        &'a mut self,
        meta: &'m RevMeta,
    ) -> (Drain<'a, T>, Drain<'a, EntryAmount<U>>) {
        match self.amounts.pre_update_check(meta) {
            PreUpdateVariant::DropLog => {
                self.truncate_future();
                let past_len = self.index;
                self.index = 0;
                return (
                    self.transitions.drain(..),
                    self.amounts.drain_past(past_len),
                );
            }
            PreUpdateVariant::DropFuture => self.truncate_future(),
            PreUpdateVariant::Nothing => {}
        }
        self.empty_drain()
    }
    pub fn pre_update_drain_future<'a, 'm>(
        &'a mut self,
        meta: &'m RevMeta,
    ) -> (Drain<'a, T>, Drain<'a, EntryAmount<U>>) {
        match self.amounts.pre_update_check(meta) {
            PreUpdateVariant::DropLog => self.truncate_past(),
            PreUpdateVariant::DropFuture => {}
            PreUpdateVariant::Nothing => return self.empty_drain(),
        }
        self.drain_future()
    }
    pub fn pre_update_drain<'a, 'm>(
        &'a mut self,
        meta: &'m RevMeta,
    ) -> (Drain<'a, T>, Drain<'a, EntryAmount<U>>, usize) {
        match self.amounts.pre_update_check(meta) {
            PreUpdateVariant::DropLog => {
                let (entry_amounts, past_len) = self.amounts.full_drain();
                self.index = 0;
                (self.transitions.drain(..), entry_amounts, past_len)
            }
            PreUpdateVariant::DropFuture => {
                let (transitions, entry_amounts) = self.drain_future();
                (transitions, entry_amounts, 0)
            }
            PreUpdateVariant::Nothing => {
                let (transitions, entry_amounts) = self.empty_drain();
                (transitions, entry_amounts, 0)
            }
        }
    }
}

/// A `&mut VecDeque<T>` wrapper that does not expose methods which remove from the deque.
pub struct LogMut<'a, T>(&'a mut VecDeque<T>);

impl<'a, T> LogMut<'a, T> {
    pub fn append(&mut self, other: &mut VecDeque<T>) {
        self.0.append(other);
    }
    pub fn push(&mut self, value: T) {
        self.0.push_back(value);
    }
}

impl<'a, T> Extend<T> for LogMut<'a, T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<'a, T> Extend<&'a T> for LogMut<'a, T>
where
    T: 'a + Copy,
{
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueEntry<T, U> {
    pub value: T,
    pub entry: U,
}

impl<'a, T: Iterator, U> IntoIterator for &'a mut ValueEntry<T, U> {
    type IntoIter = &'a mut T;
    type Item = T::Item;
    fn into_iter(self) -> Self::IntoIter {
        &mut self.value
    }
}

/// `EntryAmount` is usually encountered in draining methods of logs with multiple states/transitions per update,
/// for example [`DenseStatesLog`] which will be behind the `log` variable in the following code snippets.
///
/// These methods return two draining iterators, the first with the states/transitions of all updates that are
/// drained, and the second with `EntryAmount`, containing the entry type `U` of the log (if specified) and the
/// amount of states/transitions per update, returned by the [`amount`](Self::amount) method.
#[derive(Debug, Clone)]
pub struct EntryAmount<U> {
    pub entry: U,

    /// The amount of transitions of an update. This can be useful to chunk them.
    ///
    /// # Examples
    ///
    /// With `log` being [`&mut DenseStatesLog`](DenseStatesLog) and [`DenseStatesLog::drain_future`] returning the
    /// draining iterators, do this to chunk them by updates:
    ///
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// let (mut future_states, future_entry_amounts) = log.drain_future();
    /// ```
    ///
    /// Now iterate...
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// for entry_amount in future_entry_amounts {
    ///     let entry = entry_amount.entry;
    ///     for future_states in future_states.by_ref().take(entry_amount.amount()) {
    ///         // logic
    ///     }
    /// }
    /// ```
    ///
    /// ...or collect them:
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// let updates: Vec<(Vec<_>, _)> = future_entry_amounts.map(|entry_amount| (
    ///     future_states.by_ref().take(entry_amount.amount()).collect(),
    ///     entry_amount.entry
    /// )).collect();
    /// ```
    pub amount: usize,
}

#[cfg(test)]
mod test {
    /*
    use super::*;

    use crate::log::test::{collect_drain, collect_drain_result, collect_pop_result};

    pub(super) fn collect_pop_result<
        I1: Iterator<Item = char>,
        I2: ExactSizeIterator<Item = char>,
    >(
        actual_pop: Result<Option<ValueEntry<I1, char>>, PushedTooMany<I2, char>>,
    ) -> Result<Option<(Vec<char>, char)>, (Vec<char>, char)> {
        match actual_pop {
            Ok(None) => Ok(None),
            Ok(Some(value_entry)) => Ok(Some((value_entry.value.collect(), value_entry.entry))),
            Err(err) => Err((err.values.collect(), err.entry)),
        }
    }

    pub(super) fn collect_drain_result<
        I1: ExactSizeIterator<Item = char>,
        I2: Iterator<Item = EntryAmount<char>>,
        I3: ExactSizeIterator<Item = char>,
    >(
        actual_drain: Result<(I1, I2), PushedTooMany<I3, char>>,
    ) -> Result<Vec<(Vec<char>, char)>, (Vec<char>, char)> {
        match actual_drain {
            Ok(ok) => Ok(collect_drain(ok)),
            Err(err) => Err((err.values.collect(), err.entry)),
        }
    }

    pub(super) fn collect_drain<
        I1: ExactSizeIterator<Item = char>,
        I2: Iterator<Item = EntryAmount<char>>,
    >(
        (mut values, entry_amounts): (I1, I2),
    ) -> Vec<(Vec<char>, char)> {
        let collected = entry_amounts
            .map(|entry_amount| {
                let amount = entry_amount.amount;
                let values: Vec<_> = values.by_ref().take(amount).collect();
                assert_eq!(values.len(), amount);
                (values, entry_amount.entry)
            })
            .collect();
        assert_eq!(values.len(), 0);
        collected
    }

    #[test]
    fn serialize() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: DenseTransitionsLog<char, u8>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: DenseTransitionsLog<char, u8>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: DenseTransitionsLog<char, u8>,
        }

        let mut original = DenseTransitionsLog::new();
        original.push_and_pop_past(usize::MAX, |mut log| {
            log.extend(['a', 'b']);
            1
        });
        original.push_and_pop_past(usize::MAX, |mut log| {
            log.extend(['c', 'd']);
            2
        });
        original.backward_log().expect("in log");

        let mut logs = Logs {
            full: original.clone(),
            full_with_capacity: original.clone(),
            logless_with_capacity: original.clone(),
        };

        logs.full.entries_reserve_exact(98);
        logs.full_with_capacity.entries_reserve_exact(98);
        logs.logless_with_capacity.entries_reserve_exact(98);

        logs.full.transitions_reserve_exact(196);
        logs.full_with_capacity.transitions_reserve_exact(196);
        logs.logless_with_capacity.transitions_reserve_exact(196);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            full,
            full_with_capacity,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &DenseTransitionsLog<char, u8>,
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

        test(&full, 2, 4, false);
        test(&full_with_capacity, 2, 4, true);
        test(&logless_with_capacity, 0, 0, true);
    }

    struct Logs(Vec<[DenseTransitionsLog<char, char, 1>; 2]>);

    impl Logs {
        fn new() -> Self {
            Self(vec![Default::default()])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            push_transitions: Vec<char>,
            push_entry: char,
            expected_transitions_len: usize,
            expected_entries_len: usize,
            expected_pop_or_amount_err: Result<Option<(Vec<char>, char)>, ()>,
        ) {
            let expected_pop_or_amount_err =
                expected_pop_or_amount_err.map_err(|()| (push_transitions.clone(), push_entry));
            let expected_drained_or_amount_err = expected_pop_or_amount_err
                .clone()
                .map(|expected_drained| expected_drained.into_iter().collect::<Vec<_>>());
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = log1.try_push_and_pop_past(max_past_len, |mut logs| {
                    logs.extend(push_transitions.clone());
                    push_entry
                });
                let actual_pop = collect_pop_result(actual_pop);
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
                let actual_drain = log2.try_push_and_drain_past(max_past_len, |mut log| {
                    log.extend(push_transitions.clone());
                    push_entry
                });
                let actual_drain = collect_drain_result(actual_drain);
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
        fn forward_log(&mut self, expected_transitions: Result<(Vec<char>, char), OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transitions = log.forward_log().map(|value_entry| {
                    (
                        value_entry.value.map(|state| *state).collect::<Vec<_>>(),
                        *value_entry.entry,
                    )
                });
                assert_eq!(
                    actual_transitions, expected_transitions,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(&mut self, expected_transitions: Result<(Vec<char>, char), OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transitions = log.backward_log().map(|value_entry| {
                    (
                        value_entry.value.map(|state| *state).collect::<Vec<_>>(),
                        *value_entry.entry,
                    )
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
        logs.forward(2, vec!['a'; 2], 'A', 2, 1, Ok(None));
        logs.forward(2, vec!['b'; 3], 'B', 5, 2, Ok(None));
        // shortened log
        logs.forward(2, vec!['c'; 4], 'C', 7, 2, Ok(Some((vec!['a'; 2], 'A'))));

        logs.backward_log(Ok((vec!['c'; 4], 'C')));
        logs.backward_log(Ok((vec!['b'; 3], 'B')));
        // out of log, no mutations happend to the logs here
        logs.backward_log(Err(OutOfLog));

        logs.forward_log(Ok((vec!['b'; 3], 'B')));
        logs.forward_log(Ok((vec!['c'; 4], 'C')));
        // nothing ever logged past 'c', no mutations happend to the logs here
        logs.forward_log(Err(OutOfLog));

        logs.backward_log(Ok((vec!['c'; 4], 'C')));
        logs.backward_log(Ok((vec!['b'; 3], 'B')));

        logs.drain_future(vec![(vec!['b'; 3], 'B'), (vec!['c'; 4], 'C')], 0, 0);

        // all entries are truncated as they are in the future
        logs.forward(2, vec!['d'; 5], 'D', 5, 1, Ok(None));

        // storing too many transitions fails
        logs.forward(2, vec!['e'; 256], 'E', 5, 1, Err(()));
    }
    */
}
