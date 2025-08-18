use std::{
    any::TypeId, collections::{
        vec_deque::{Drain, IntoIter, Iter}, TryReserveError, VecDeque
    }, fmt::{Debug, Display}, ops::{Deref, Range}
};

use bevy::{reflect::Reflect, utils::default};

use super::{
    EntryAmount, LogMut, OutOfLog, PushedTooMany, SparseDrain, SparseStateLog, USIZE_BYTES,
    ValueEntry,
};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SparseStatesLog<T, U = (), const AMOUNT_BYTES: usize = USIZE_BYTES> {
    amounts: SparseStateLog<EntryAmount<U, AMOUNT_BYTES>>,
    states: VecDeque<T>,
    index: usize,
}

impl<T: Display, U: Display + 'static, const AMOUNT_BYTES: usize> Display for SparseStatesLog<T, U, AMOUNT_BYTES> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let entry_amount = &*self.amounts;
        let range = self.index..(self.index + entry_amount.amount());
        let write_u = TypeId::of::<U>() != TypeId::of::<()>();
        if write_u {
            write!(f, "({}, ", entry_amount.entry)?;
        }
        write!(f, "[")?;
        let mut iter = self.states.range(range);
        if let Some(first) = iter.next() {
            write!(f, "{first}")?;
        }
        for state in iter {
            write!(f, ", {state}")?;
        }
        write!(f, "]")?;
        if write_u {
            write!(f, ")")?;
        }
        Ok(())
    }
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{
        LoglessState, LoglessWithCapacity, WithCapacity, WithCapacityWrapper, WithRange,
    };

    use super::{EntryAmount, SparseStateLog, SparseStatesLog};

    impl<T, U, const AMOUNT_BYTES: usize> LoglessState for SparseStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (&'se EntryAmount<U, AMOUNT_BYTES>, WithRange<'se, T>);
        type De = (EntryAmount<U, AMOUNT_BYTES>, VecDeque<T>);
        fn get_logless_state(&self) -> Self::Se<'_> {
            (
                self.amounts.get_logless_state(),
                WithRange {
                    deque: &self.states,
                    range: self.get_range_entry().0,
                },
            )
        }
        fn from_logless_state((amounts, states): Self::De) -> Self {
            let index = states.len();
            Self {
                amounts: amounts.into(),
                states,
                index,
            }
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for SparseStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (
            <SparseStateLog<EntryAmount<U, AMOUNT_BYTES>> as WithCapacity>::Se<'se>,
            WithCapacityWrapper<&'se VecDeque<T>>,
            usize,
        );
        type De = (
            <SparseStateLog<EntryAmount<U, AMOUNT_BYTES>> as WithCapacity>::De,
            WithCapacityWrapper<VecDeque<T>>,
            usize,
        );
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                WithCapacity::get_with_capacity(&self.amounts),
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

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity for SparseStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (
            <SparseStateLog<EntryAmount<U, AMOUNT_BYTES>> as LoglessWithCapacity>::Se<'se>,
            WithCapacityWrapper<WithRange<'se, T>>,
        );
        type De = (
            <SparseStateLog<EntryAmount<U, AMOUNT_BYTES>> as LoglessWithCapacity>::De,
            WithCapacityWrapper<VecDeque<T>>,
        );
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (
                self.amounts.get_logless_with_capacity(),
                WithCapacityWrapper(WithRange {
                    deque: &self.states,
                    range: self.get_range_entry().0,
                }),
            )
        }
        fn from_logless_with_capacity((amounts, WithCapacityWrapper(states)): Self::De) -> Self {
            let index = states.len();
            Self {
                amounts: SparseStateLog::from_logless_with_capacity(amounts),
                states,
                index,
            }
        }
    }
}

impl<T, U: Default, const AMOUNT_BYTES: usize> Default for SparseStatesLog<T, U, AMOUNT_BYTES> {
    fn default() -> Self {
        Self::new_empty(default())
    }
}

impl<T, U, const AMOUNT_BYTES: usize> Deref for SparseStatesLog<T, U, AMOUNT_BYTES> {
    type Target = U;
    fn deref(&self) -> &Self::Target {
        &self.amounts.entry
    }
}

impl<T, U, const AMOUNT_BYTES: usize> SparseStatesLog<T, U, AMOUNT_BYTES> {
    pub fn new(states: impl IntoIterator<Item = T>, entry: U) -> Self {
        Self::try_new(states, entry).unwrap_or_else(|err| panic!("{err}"))
    }
    pub fn try_new(
        states: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, PushedTooMany<IntoIter<T>, U, AMOUNT_BYTES>> {
        let states = VecDeque::from_iter(states.into_iter());
        let pushed_amount = states.len();
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if AMOUNT_BYTES < USIZE_BYTES && pushed_amount != entry_amount.amount() {
            return Err(PushedTooMany {
                values: states.into_iter(),
                entry: entry_amount.entry,
            });
        }
        Ok(Self {
            amounts: SparseStateLog::new(entry_amount),
            states,
            index: pushed_amount,
        })
    }
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: SparseStateLog::new(EntryAmount::zero(entry)),
            states: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(
        states: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Self {
        Self::try_with_capacities(states, entry, states_capacity, entries_capacity)
            .unwrap_or_else(|err| panic!("{err}"))
    }
    pub fn try_with_capacities(
        states: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Result<Self, PushedTooMany<IntoIter<T>, U, AMOUNT_BYTES>> {
        let mut states_deque = VecDeque::with_capacity(states_capacity);
        states_deque.extend(states.into_iter());
        let pushed_amount = states_deque.len();
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if AMOUNT_BYTES < USIZE_BYTES && pushed_amount != entry_amount.amount() {
            return Err(PushedTooMany {
                values: states_deque.into_iter(),
                entry: entry_amount.entry,
            });
        }
        Ok(Self {
            amounts: SparseStateLog::with_capacity(entry_amount, entries_capacity),
            states: states_deque,
            index: pushed_amount,
        })
    }
    pub fn with_capacities_empty(
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Self {
        Self {
            amounts: SparseStateLog::with_capacity(EntryAmount::zero(entry), entries_capacity),
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
    fn get_range_entry(&self) -> (Range<usize>, &U) {
        let amount = self.amounts.amount();
        let from = self.index - amount;
        (from..self.index, &self.amounts.entry)
    }
    pub fn get(&self) -> (Iter<T>, &U) {
        let (range, entry) = self.get_range_entry();
        let states = self.states.range(range);
        (states, entry)
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
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if AMOUNT_BYTES < USIZE_BYTES && pushed_amount != entry_amount.amount() {
            return Err(PushedTooMany {
                values: self.states.drain(self.index..),
                entry: entry_amount.entry,
            });
        }
        self.index = self.states.len();
        let pop = self
            .amounts
            .push_and_pop_past(max_past_len, Some(entry_amount))
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
    pub fn push_none_and_pop_past(
        &mut self,
        max_past_len: usize,
    ) -> Option<ValueEntry<Drain<T>, U>> {
        self.states.truncate(self.index);
        self.amounts
            .push_and_pop_past(max_past_len, None)
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                self.index -= amount;
                ValueEntry {
                    value: self.states.drain(..amount),
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
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if AMOUNT_BYTES < USIZE_BYTES && pushed_amount != entry_amount.amount() {
            return Err(PushedTooMany {
                values: self.states.drain(self.index..),
                entry: entry_amount.entry,
            });
        }
        self.index = self.states.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, Some(entry_amount));
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain
            .map(|entry_amount| entry_amount.value.amount())
            .sum();
        self.index -= amount;
        Ok((
            self.states.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        ))
    }
    pub fn push_none_and_drain_past(
        &mut self,
        max_past_len: usize,
    ) -> (Drain<T>, SparseDrain<EntryAmount<U, AMOUNT_BYTES>>) {
        self.states.truncate(self.index);
        let to_drain = self.amounts.push_and_iter_to_drain_past(max_past_len, None);
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain
            .map(|entry_amount| entry_amount.value.amount())
            .sum();
        self.index -= amount;
        (
            self.states.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        )
    }
    pub fn drain_future(&mut self) -> (Drain<T>, SparseDrain<EntryAmount<U, AMOUNT_BYTES>>) {
        (self.states.drain(self.index..), self.amounts.drain_future())
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        let amount = self.amounts.amount();
        self.states.truncate(self.index);
        self.states.drain(..self.index - amount);
        self.index = amount;
    }
    pub fn clear_with(&mut self, states: impl IntoIterator<Item = T>, entry: U) {
        self.try_clear_with(states, entry)
            .unwrap_or_else(|err| panic!("{err}"))
    }
    pub fn try_clear_with(
        &mut self,
        states: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), PushedTooMany<IntoIter<T>, U, AMOUNT_BYTES>> {
        let mut states = VecDeque::from_iter(states.into_iter());
        let pushed_amount = states.len();
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            return Err(PushedTooMany {
                values: states.into_iter(),
                entry: entry_amount.entry,
            });
        }
        self.states.clear();
        self.states.append(&mut states);
        self.amounts.clear_with(entry_amount);
        self.index = pushed_amount;
        Ok(())
    }
    pub fn clear_empty(&mut self, entry: U) {
        self.states.clear();
        self.amounts.clear_with(EntryAmount::zero(entry));
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<bool, OutOfLog> {
        let amount = self.amounts.amount();
        if self.amounts.backward_log()? {
            self.index -= amount;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn forward_log(&mut self) -> Result<bool, OutOfLog> {
        if self.amounts.forward_log()? {
            self.index += self.amounts.amount();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod test {
    use std::usize;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::log::test::{collect_drain, collect_drain_result, collect_pop_result};

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: SparseStatesLog<char, u8>,
            #[serde(with = "crate::log::logless_state")]
            logless: SparseStatesLog<char, u8>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: SparseStatesLog<char, u8>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: SparseStatesLog<char, u8>,
        }

        let mut original = SparseStatesLog::<char, u8>::new(['a', 'b'], 1);
        original.push_some_and_pop_past(usize::MAX, |mut log| {
            log.extend(['c', 'd']);
            2
        });
        original.push_some_and_pop_past(usize::MAX, |mut log| {
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

        let test = |log: &SparseStatesLog<char, u8>, entries_len, states_len, with_capacity| {
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
        let mut original = SparseStatesLog::<_, _, 1>::try_new([1, 1], 'a').unwrap();
        original
            .try_push_some_and_pop_past(usize::MAX, |mut log| {
                log.extend([2, 2]);
                'b'
            })
            .unwrap();
        original.push_none_and_pop_past(usize::MAX);
        original
            .try_push_some_and_pop_past(usize::MAX, |mut log| {
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
            err.values.collect::<Vec<_>>(),
            vec![0; 256],
            "log: {log:#?}\noriginal: {original:#?}"
        );
        assert_eq!(err.entry, 'e', "log: {log:#?}\noriginal: {original:#?}");
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

    struct Logs(Vec<[SparseStatesLog<char, char, 1>; 2]>);

    impl Logs {
        fn new(states: Vec<char>, entry: char) -> Self {
            let log = SparseStatesLog::try_new(states, entry).unwrap();
            Logs(vec![[log.clone(), log]])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            states: Vec<char>,
            entry: char,
            push: bool,
            expected_states_len: usize,
            expected_entries_len: usize,
            expected_pop_or_err_state: Result<Option<(Vec<char>, char)>, (Vec<char>, char)>,
        ) {
            let expected_drain_or_err_state = expected_pop_or_err_state
                .clone()
                .map(|drain| drain.into_iter().collect::<Vec<_>>());
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = if push {
                    log1.try_push_some_and_pop_past(max_past_len, |mut log| {
                        log.extend(states.clone());
                        entry
                    })
                } else {
                    Ok(log1.push_none_and_pop_past(max_past_len))
                };
                let actual_pop = match collect_pop_result(actual_pop) {
                    Ok(ok) => {
                        let actual_get = log1.get();
                        let actual_get = (actual_get.0.cloned().collect::<Vec<_>>(), *actual_get.1);
                        assert_eq!(
                            actual_get,
                            (states.clone(), entry),
                            "\nbefore: {before:#?}\nafter: {log1:#?}"
                        );
                        Ok(ok)
                    }
                    Err(err) => {
                        assert_eq!(
                            err,
                            (states.clone(), entry),
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
                let actual_drain = if push {
                    log2.try_push_some_and_drain_past(max_past_len, |mut log| {
                        log.extend(states.clone());
                        entry
                    })
                } else {
                    Ok(log2.push_none_and_drain_past(max_past_len))
                };
                let actual_drain = match collect_drain_result(actual_drain) {
                    Ok(ok) => {
                        let actual_get = log2.get();
                        let actual_get = (actual_get.0.cloned().collect::<Vec<_>>(), *actual_get.1);
                        assert_eq!(
                            actual_get,
                            (states.clone(), entry),
                            "\nbefore: {before:#?}\nafter: {log2:#?}"
                        );
                        Ok(ok)
                    }
                    Err(err) => {
                        assert_eq!(
                            err,
                            (states.clone(), entry),
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
            expected_result: Result<bool, OutOfLog>,
        ) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_result = log.forward_log();
                assert_eq!(
                    actual_result, expected_result,
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
            expected_result: Result<bool, OutOfLog>,
        ) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_result = log.backward_log();
                assert_eq!(
                    actual_result, expected_result,
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
        logs.forward(2, vec!['a'; 2], 'A', false, 2, 0, Ok(None));
        logs.forward(2, vec!['b'; 3], 'B', true, 5, 1, Ok(None));
        // does not pop yet because the skip after the initial state is still in log range
        logs.forward(2, vec!['c'; 4], 'C', true, 9, 2, Ok(None));
        // shortened log
        logs.forward(
            2,
            vec!['c'; 4],
            'C',
            false,
            7,
            1,
            Ok(Some((vec!['a'; 2], 'A'))),
        );

        logs.backward_log(vec!['c'; 4], 'C', Ok(false));
        logs.backward_log(vec!['b'; 3], 'B', Ok(true));
        // out of log, no mutations happend to the logs here
        logs.backward_log(vec!['b'; 3], 'B', Err(OutOfLog));

        logs.forward_log(vec!['c'; 4], 'C', Ok(true));
        logs.forward_log(vec!['c'; 4], 'C', Ok(false));
        // nothing ever logged past 'c', no mutations happend to the logs here
        logs.forward_log(vec!['c'; 4], 'C', Err(OutOfLog));

        logs.backward_log(vec!['c'; 4], 'C', Ok(false));
        logs.backward_log(vec!['b'; 3], 'B', Ok(true));

        logs.drain_future(vec![(vec!['c'; 4], 'C')], 3, 0);

        // all entries are truncated as they are in the future
        logs.forward(2, vec!['b'; 3], 'B', false, 3, 0, Ok(None));

        // storing too many states fails
        logs.forward(2, vec!['d'; 256], 'D', true, 3, 0, Err((vec!['b'; 3], 'B')));
    }
}
