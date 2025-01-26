use std::{
    collections::{
        vec_deque::{Drain, Iter},
        TryReserveError, VecDeque,
    },
    fmt::Debug,
    ops::{Deref, Range},
};

use bevy::reflect::Reflect;

use super::{AmountErr, DenseStateLog, EntryAmount, LogMut, OutOfLog, ValueEntry, USIZE_BYTES};

#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DenseStatesLog<T, U = (), const AMOUNT_BYTES: usize = USIZE_BYTES> {
    amounts: DenseStateLog<EntryAmount<U, AMOUNT_BYTES>>,
    states: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{
        LoglessState, LoglessWithCapacity, WithCapacity, WithCapacityWrapper, WithRange,
    };

    use super::{DenseStateLog, DenseStatesLog, EntryAmount};

    impl<T, U, const AMOUNT_BYTES: usize> LoglessState for DenseStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (&'se EntryAmount<U, AMOUNT_BYTES>, WithRange<'se, T>);
        type De = (EntryAmount<U, AMOUNT_BYTES>, VecDeque<T>);
        fn get_logless_state(&self) -> Self::Se<'_> {
            let (entry, range) = self.get_entry_range();
            (
                entry,
                WithRange {
                    deque: &self.states,
                    range,
                },
            )
        }
        fn from_logless_state((entry, states): Self::De) -> Self {
            let index = entry.amount();
            Self {
                amounts: entry.into(),
                states,
                index,
            }
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for DenseStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (
            <DenseStateLog<EntryAmount<U, AMOUNT_BYTES>> as WithCapacity>::Se<'se>,
            WithCapacityWrapper<&'se VecDeque<T>>,
            usize,
        );
        type De = (
            <DenseStateLog<EntryAmount<U, AMOUNT_BYTES>> as WithCapacity>::De,
            WithCapacityWrapper<VecDeque<T>>,
            usize,
        );
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                self.amounts.get_with_capacity(),
                WithCapacityWrapper(&self.states),
                self.index,
            )
        }
        fn from_with_capacity((amounts, WithCapacityWrapper(states), index): Self::De) -> Self {
            Self {
                amounts: WithCapacity::from_with_capacity(amounts),
                states,
                index,
            }
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity for DenseStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (
            <DenseStateLog<EntryAmount<U, AMOUNT_BYTES>> as LoglessWithCapacity>::Se<'se>,
            WithCapacityWrapper<WithRange<'se, T>>,
        );
        type De = (
            <DenseStateLog<EntryAmount<U, AMOUNT_BYTES>> as LoglessWithCapacity>::De,
            WithCapacityWrapper<VecDeque<T>>,
        );
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (
                self.amounts.get_logless_with_capacity(),
                WithCapacityWrapper(WithRange {
                    deque: &self.states,
                    range: self.get_entry_range().1,
                }),
            )
        }
        fn from_logless_with_capacity((amounts, WithCapacityWrapper(states)): Self::De) -> Self {
            let amounts = DenseStateLog::from_logless_with_capacity(amounts);
            let index = amounts.amount();
            Self {
                amounts,
                states,
                index,
            }
        }
    }
}

impl<T, U, const AMOUNT_BYTES: usize> Deref for DenseStatesLog<T, U, AMOUNT_BYTES> {
    type Target = U;
    fn deref(&self) -> &Self::Target {
        &self.amounts.entry
    }
}

impl<T, U, const AMOUNT_BYTES: usize> DenseStatesLog<T, U, AMOUNT_BYTES> {
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: DenseStateLog::new(EntryAmount::zero(entry)),
            states: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities_empty(
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Self {
        Self {
            amounts: DenseStateLog::with_capacity(EntryAmount::zero(entry), entries_capacity),
            states: VecDeque::with_capacity(states_capacity),
            index: 0,
        }
    }
    pub fn entries_len(&self) -> usize {
        self.amounts.states_len()
    }
    pub fn states_len(&self) -> usize {
        self.states.len()
    }
    pub fn entries_capacity(&self) -> usize {
        self.amounts.states_capacity()
    }
    pub fn states_capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn entries_is_empty(&self) -> bool {
        self.amounts.states_is_empty()
    }
    pub fn states_is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn entries_reserve(&mut self, additional: usize) {
        self.amounts.states_reserve(additional)
    }
    pub fn states_reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn entries_reserve_exact(&mut self, additional: usize) {
        self.amounts.states_reserve_exact(additional)
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn entries_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.states_try_reserve(additional)
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn entries_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.states_try_reserve_exact(additional)
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn entries_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.states_shrink_to(min_capacity)
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn entries_shrink_to_fit(&mut self) {
        self.amounts.states_shrink_to_fit()
    }
    pub fn states_shrink_to_fit(&mut self) {
        self.states.shrink_to_fit()
    }
    fn get_entry_range(&self) -> (&EntryAmount<U, AMOUNT_BYTES>, Range<usize>) {
        let entry_amount = &self.amounts;
        let amount = entry_amount.amount();
        let from = self.index - amount;
        (&entry_amount, from..self.index)
    }
    pub fn get(&self) -> (Iter<T>, &U) {
        let (entry, range) = self.get_entry_range();
        let states = self.states.range(range);
        (states, &entry.entry)
    }
    pub fn drain_future(&mut self) -> (Drain<T>, Drain<EntryAmount<U, AMOUNT_BYTES>>) {
        (self.states.drain(self.index..), self.amounts.drain_future())
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        let amount = self.amounts.amount();
        self.states.truncate(self.index);
        self.states.drain(..self.index - amount);
        self.index = amount;
    }
    pub fn clear_empty(&mut self, entry: U) {
        self.states.clear();
        self.amounts.clear_with(EntryAmount::zero(entry));
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        let amount = self.amounts.amount();
        self.amounts.backward_log()?;
        self.index -= amount;
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        self.amounts.forward_log()?;
        let amount = self.amounts.amount();
        self.index += amount;
        Ok(())
    }
}

impl<T, U> DenseStatesLog<T, U, USIZE_BYTES> {
    pub fn new(states: impl IntoIterator<Item = T>, entry: U) -> Self {
        let states = VecDeque::from_iter(states);
        let amount = states.len();
        let entry_amount = EntryAmount::new(entry, amount);
        let amounts = DenseStateLog::new(entry_amount);
        Self {
            amounts,
            states,
            index: amount,
        }
    }
    pub fn with_capacities(
        states: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Self {
        let mut states_deque = VecDeque::with_capacity(states_capacity);
        states_deque.extend(states);
        let amount = states_deque.len();
        let entry_amount = EntryAmount::new(entry, amount);
        let amounts = DenseStateLog::with_capacity(entry_amount, entries_capacity);
        Self {
            amounts,
            states: states_deque,
            index: amount,
        }
    }
    pub fn push<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        self.index = self.states.len();
        self.amounts.push(entry_amount);
    }
    pub fn push_and_pop_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Option<ValueEntry<Drain<T>, U>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        self.index = self.states.len();
        self.amounts
            .push_and_pop_past(max_past_len, entry_amount)
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                self.index -= amount;
                ValueEntry {
                    value: self.states.drain(..amount),
                    entry: entry_amount.entry,
                }
            })
    }
    pub fn push_and_drain_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> (Drain<T>, Drain<EntryAmount<U>>) {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        self.index = self.states.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, entry_amount);
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain.map(|entry_amount| entry_amount.amount()).sum();
        self.index -= amount;
        (
            self.states.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        )
    }
}

// todo: bound AMOUNT_BYTES to 1..USIZE_BYTES
impl<T, U, const AMOUNT_BYTES: usize> DenseStatesLog<T, U, AMOUNT_BYTES> {
    pub fn try_new(
        states: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<VecDeque<T>, U, AMOUNT_BYTES>> {
        let states = VecDeque::from_iter(states);
        let pushed_amount = states.len();
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            return Err(AmountErr {
                values: states,
                entry_amount,
            });
        }
        Ok(Self {
            amounts: DenseStateLog::new(entry_amount),
            states,
            index: pushed_amount,
        })
    }
    pub fn try_with_capacities(
        states: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Result<Self, AmountErr<VecDeque<T>, U, AMOUNT_BYTES>> {
        let mut states_deque = VecDeque::with_capacity(states_capacity);
        states_deque.extend(states);
        let pushed_amount = states_deque.len();
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            return Err(AmountErr {
                values: states_deque,
                entry_amount,
            });
        }
        let amounts = DenseStateLog::with_capacity(entry_amount, entries_capacity);
        Ok(Self {
            amounts,
            states: states_deque,
            index: pushed_amount,
        })
    }
    pub fn try_push<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<Drain<T>, U, AMOUNT_BYTES>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            let values = self.states.drain(self.index..);
            return Err(AmountErr {
                values,
                entry_amount,
            });
        }
        self.index = self.states.len();
        Ok(())
    }
    pub fn try_push_and_pop_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<ValueEntry<Drain<T>, U>>, AmountErr<Drain<T>, U, AMOUNT_BYTES>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            let values = self.states.drain(self.index..);
            return Err(AmountErr {
                values,
                entry_amount,
            });
        }
        self.index = self.states.len();
        let pop = self
            .amounts
            .push_and_pop_past(max_past_len, entry_amount)
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                self.index -= amount;
                ValueEntry {
                    value: self.states.drain(..amount),
                    entry: entry_amount.entry,
                }
            });
        Ok(pop)
    }
    pub fn try_push_and_drain_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(Drain<T>, Drain<EntryAmount<U, AMOUNT_BYTES>>), AmountErr<Drain<T>, U, AMOUNT_BYTES>>
    {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            let values = self.states.drain(self.index..);
            return Err(AmountErr {
                values,
                entry_amount,
            });
        }
        self.index = self.states.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, entry_amount);
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain.map(|entry_amount| entry_amount.amount()).sum();
        self.index -= amount;
        Ok((
            self.states.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        ))
    }
    pub fn try_clear_with(
        &mut self,
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), AmountErr<VecDeque<T>, U, AMOUNT_BYTES>> {
        let mut states = VecDeque::from_iter(iter);
        let pushed_amount = states.len();
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            return Err(AmountErr {
                values: states,
                entry_amount,
            });
        }
        self.states.clear();
        self.states.append(&mut states);
        self.amounts.clear_with(entry_amount);
        self.index = pushed_amount;
        Ok(())
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
            full: DenseStatesLog<char, u8>,
            #[serde(with = "crate::log::logless_state")]
            logless: DenseStatesLog<char, u8>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: DenseStatesLog<char, u8>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: DenseStatesLog<char, u8>,
        }

        let mut original = DenseStatesLog::new(['a', 'b'], 1);
        original.push(|mut log| {
            log.extend(['c', 'd']);
            2
        });
        original.push(|mut log| {
            log.extend(['e', 'f']);
            3
        });
        original.backward_log().expect("in log");

        let mut logs = Logs {
            full: original.clone(),
            logless: original.clone(),
            full_with_capacity: original.clone(),
            logless_with_capacity: original.clone(),
        };

        logs.full.entries_reserve_exact(98);
        logs.logless.entries_reserve_exact(98);
        logs.full_with_capacity.entries_reserve_exact(98);
        logs.logless_with_capacity.entries_reserve_exact(98);

        logs.full.states_reserve_exact(194);
        logs.logless.states_reserve_exact(194);
        logs.full_with_capacity.states_reserve_exact(194);
        logs.logless_with_capacity.states_reserve_exact(194);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            full,
            logless,
            full_with_capacity,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &DenseStatesLog<char, u8>, entries_len, states_len, with_capacity| {
            let (states, entry) = log.get();
            let states: Vec<_> = states.cloned().collect();

            assert_eq!(
                states,
                vec!['c', 'd'],
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                *entry, 2,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.entries_len(),
                entries_len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.states_len(),
                states_len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.entries_capacity() >= 100,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.entries_capacity()
            );
            assert_eq!(
                log.states_capacity() >= 200,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.states_capacity()
            );
        };

        test(&full, 2, 6, false);
        test(&logless, 0, 2, false);
        test(&full_with_capacity, 2, 6, true);
        test(&logless_with_capacity, 0, 2, true);
    }

    #[test]
    fn clear() {
        let mut original = DenseStatesLog::<_, _, 1>::try_new([1, 1], 'a').unwrap();
        original
            .try_push(|mut log| {
                log.extend([2, 2]);
                'b'
            })
            .unwrap();
        original
            .try_push(|mut log| {
                log.extend([3, 3]);
                'c'
            })
            .unwrap();
        original.backward_log().expect("in log");

        let mut log = original.clone();
        log.clear();
        let state = log.get();
        assert_eq!(
            state.0.cloned().collect::<Vec<_>>(),
            [2, 2],
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(*state.1, 'b', "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            2,
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(
            log.entries_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );

        let mut log = original.clone();
        log.clear_empty('d');
        let mut state = log.get();
        assert_eq!(
            state.0.next(),
            None,
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(*state.1, 'd', "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(
            log.entries_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );

        let mut log = original.clone();
        let err = log
            .try_clear_with([0; 256], 'e')
            .expect_err("pushed too many");
        let state = log.get();
        assert_eq!(
            err.values, [0; 256],
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(
            err.entry_amount.amount(),
            256,
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(
            err.max_amount(),
            255,
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(
            err.entry_amount.entry, 'e',
            "log: {log:#?}\noriginal: {original:#?}"
        );
        // unchanged
        assert_eq!(
            state.0.cloned().collect::<Vec<_>>(),
            [2, 2],
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(*state.1, 'b', "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            6,
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(
            log.entries_len(),
            2,
            "log: {log:#?}\noriginal: {original:#?}"
        );

        let mut log = original.clone();
        let result = log.try_clear_with([4, 4], 'f');
        let state = log.get();
        assert!(result.is_ok(), "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            state.0.cloned().collect::<Vec<_>>(),
            [4, 4],
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(*state.1, 'f', "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            2,
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(
            log.entries_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );
    }

    struct Logs(Vec<[DenseStatesLog<char, char, 1>; 2]>);

    impl Logs {
        fn new(states: Vec<char>, entry: char) -> Self {
            let log = DenseStatesLog::try_new(states, entry).unwrap();
            Self(vec![[log.clone(), log]])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            push_states: Vec<char>,
            push_entry: char,
            expected_states_len: usize,
            expected_entries_len: usize,
            expected_pop_or_err_state: Result<Option<(Vec<char>, char)>, (Vec<char>, char)>,
        ) {
            let expected_drain_or_err_state = expected_pop_or_err_state
                .clone()
                .map(|drain| drain.into_iter().collect::<Vec<_>>());
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = log1.try_push_and_pop_past(max_past_len, |mut log| {
                    log.extend(push_states.clone());
                    push_entry
                });
                let actual_pop = match collect_pop_result(actual_pop) {
                    Ok(ok) => {
                        let actual_get = log1.get();
                        let actual_get = (actual_get.0.cloned().collect::<Vec<_>>(), *actual_get.1);
                        assert_eq!(
                            actual_get,
                            (push_states.clone(), push_entry),
                            "\nbefore: {before:#?}\nafter: {log1:#?}"
                        );
                        Ok(ok)
                    }
                    Err(err) => {
                        assert_eq!(
                            err,
                            (push_states.clone(), push_entry),
                            "\nbefore: {before:#?}\nafter: {log1:#?}"
                        );
                        let actual_get = log1.get();
                        Err((actual_get.0.cloned().collect::<Vec<_>>(), *actual_get.1))
                    }
                };
                assert_eq!(
                    actual_pop,
                    expected_pop_or_err_state.clone(),
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );
                assert_eq!(
                    log1.states_len(),
                    expected_states_len,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );
                assert_eq!(
                    log1.entries_len(),
                    expected_entries_len,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );

                let before = log2.clone();
                let actual_drain = log2.try_push_and_drain_past(max_past_len, |mut log| {
                    log.extend(push_states.clone());
                    push_entry
                });
                let actual_drain = match collect_drain_result(actual_drain) {
                    Ok(ok) => {
                        let actual_get = log2.get();
                        let actual_get = (actual_get.0.cloned().collect::<Vec<_>>(), *actual_get.1);
                        assert_eq!(
                            actual_get,
                            (push_states.clone(), push_entry),
                            "\nbefore: {before:#?}\nafter: {log2:#?}"
                        );
                        Ok(ok)
                    }
                    Err(err) => {
                        assert_eq!(
                            err,
                            (push_states.clone(), push_entry),
                            "\nbefore: {before:#?}\nafter: {log2:#?}"
                        );
                        let actual_get = log2.get();
                        Err((actual_get.0.cloned().collect::<Vec<_>>(), *actual_get.1))
                    }
                };
                assert_eq!(
                    actual_drain,
                    expected_drain_or_err_state.clone(),
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
                assert_eq!(
                    log2.states_len(),
                    expected_states_len,
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
            expected_states: Vec<char>,
            expected_entry: char,
            expected_out_of_log: bool,
        ) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_out_of_log = log.forward_log().is_err();
                assert_eq!(
                    actual_out_of_log, expected_out_of_log,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
                let (actual_states, actual_entry) = log.get();
                let actual_states: Vec<_> = actual_states.cloned().collect();
                assert_eq!(
                    actual_states, expected_states,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
                assert_eq!(
                    *actual_entry, expected_entry,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(
            &mut self,
            expected_states: Vec<char>,
            expected_entry: char,
            expected_out_of_log: bool,
        ) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_out_of_log = log.backward_log().is_err();
                let (actual_states, actual_entry) = log.get();
                let actual_states: Vec<_> = actual_states.cloned().collect();
                assert_eq!(
                    actual_out_of_log, expected_out_of_log,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
                assert_eq!(
                    actual_states, expected_states,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
                assert_eq!(
                    *actual_entry, expected_entry,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn drain_future(
            &mut self,
            expected_future: Vec<(Vec<char>, char)>,
            expected_states_len: usize,
            expected_entries_len: usize,
        ) {
            self.0 = std::mem::take(&mut self.0)
                .into_iter()
                .flatten()
                .map(|mut log| {
                    let before = log.clone();
                    let actual_future = collect_drain(log.drain_future());
                    assert_eq!(
                        actual_future, expected_future,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    assert_eq!(
                        log.states_len(),
                        expected_states_len,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    assert_eq!(
                        log.entries_len(),
                        expected_entries_len,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    [before, log]
                })
                .collect();
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut logs = Logs::new(vec!['a'; 2], 'A');
        logs.forward(2, vec!['b'; 3], 'B', 5, 1, Ok(None));
        logs.forward(2, vec!['c'; 4], 'C', 9, 2, Ok(None));
        // shortened log
        logs.forward(2, vec!['d'; 5], 'D', 12, 2, Ok(Some((vec!['a'; 2], 'A'))));

        logs.backward_log(vec!['c'; 4], 'C', false);
        logs.backward_log(vec!['b'; 3], 'B', false);
        // out of log, no mutations happend the logs here
        logs.backward_log(vec!['b'; 3], 'B', true);

        logs.forward_log(vec!['c'; 4], 'C', false);
        logs.forward_log(vec!['d'; 5], 'D', false);
        // nothing ever logged past 'D', no mutations happend to the logs here
        logs.forward_log(vec!['d'; 5], 'D', true);

        logs.backward_log(vec!['c'; 4], 'C', false);
        logs.backward_log(vec!['b'; 3], 'B', false);

        logs.drain_future(vec![(vec!['c'; 4], 'C'), (vec!['d'; 5], 'D')], 3, 0);

        // all entries are truncated as they are in the future
        logs.forward(2, vec!['e'; 6], 'E', 9, 1, Ok(None));

        // storing too many states fails
        logs.forward(2, vec!['f'; 256], 'F', 9, 1, Err((vec!['e'; 6], 'E')));
    }
}
