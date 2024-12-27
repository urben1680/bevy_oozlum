use std::{
    collections::{
        vec_deque::{Drain, Iter},
        TryReserveError, VecDeque,
    },
    convert::Infallible,
    fmt::Debug,
    ops::{Deref, Range},
};

use bevy::{reflect::Reflect, utils::default};

use crate::meta::RevMeta;

use super::{
    doc_with_amount, impl_with_amount, AmountErr, EntryAmount, LogMut, LoggedAt, NotUSize,
    OutOfLog, RareDrain, RareStateLog, ValueEntry, WithAmountInternal,
};

#[doc = doc_with_amount!(struct)]
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareStatesLog<T, U = (), const AMOUNT_BYTES: usize = 0>
where
    Self: WithAmountInternal<Entry = U>,
{
    amounts: RareStateLog<EntryAmount<Self>>,
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

    use super::{EntryAmount, RareStateLog, RareStatesLog, WithAmountInternal};

    impl<T, U, const AMOUNT_BYTES: usize> LoglessState for RareStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmountInternal<Entry = U>,
    {
        type Se<'se> = (&'se EntryAmount<Self>, WithRange<'se, T>);
        type De = (EntryAmount<Self>, VecDeque<T>);
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

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for RareStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmountInternal<Entry = U>,
    {
        type Se<'se> = (
            <RareStateLog<EntryAmount<Self>> as WithCapacity>::Se<'se>,
            WithCapacityWrapper<&'se VecDeque<T>>,
            usize,
        );
        type De = (
            <RareStateLog<EntryAmount<Self>> as WithCapacity>::De,
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

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity for RareStatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmountInternal<Entry = U>,
    {
        type Se<'se> = (
            <RareStateLog<EntryAmount<Self>> as LoglessWithCapacity>::Se<'se>,
            WithCapacityWrapper<WithRange<'se, T>>,
        );
        type De = (
            <RareStateLog<EntryAmount<Self>> as LoglessWithCapacity>::De,
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
                amounts: RareStateLog::from_logless_with_capacity(amounts),
                states,
                index,
            }
        }
    }
}

impl_with_amount!(RareStatesLog);

#[doc = doc_with_amount!(impl)]
impl<T, U: Default, const AMOUNT_BYTES: usize> Default for RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    fn default() -> Self {
        Self::new_empty(default())
    }
}

#[doc = doc_with_amount!(impl)]
impl<T, U, const AMOUNT_BYTES: usize> From<U> for RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    fn from(entry: U) -> Self {
        Self::new_empty(entry)
    }
}

#[doc = doc_with_amount!(impl where Infallible)]
impl<T, U: Default, const AMOUNT_BYTES: usize> FromIterator<T> for RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U, Err = Infallible>,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::new(iter, default())
    }
}

#[doc = doc_with_amount!(impl)]
impl<T, U, const AMOUNT_BYTES: usize> Deref for RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    type Target = U;
    fn deref(&self) -> &Self::Target {
        &self.amounts.entry
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: RareStateLog::new(EntryAmount::zero(entry)),
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
            amounts: RareStateLog::with_capacity(EntryAmount::zero(entry), entries_capacity),
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
    pub fn drain_future(&mut self) -> (Drain<T>, RareDrain<EntryAmount<Self>>) {
        (self.states.drain(self.index..), self.amounts.drain_future())
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        let amount = self.amounts.amount;
        let amount = <Self as WithAmountInternal>::amount_to_usize(amount);
        self.states.truncate(self.index);
        self.states.drain(..self.index - amount);
        self.index = amount;
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
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<ValueEntry<Drain<T>, U>> {
        self.amounts
            .pop_past_by_len(max_past_len)
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> Drain<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_len(max_past_len)
            .map(|entry_amount| entry_amount.amount())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
    }
    fn drain_past_by_amount(&mut self, entry_amount: EntryAmount<Self>) -> ValueEntry<Drain<T>, U> {
        let amount = entry_amount.amount();
        self.index -= amount;
        ValueEntry {
            value: self.states.drain(..amount),
            entry: entry_amount.entry,
        }
    }
    fn fallible_new(
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<VecDeque<T>, Self>> {
        let states = VecDeque::from_iter(iter.into_iter());
        let pushed_amount = states.len();
        match <Self as WithAmountInternal>::usize_to_amount(pushed_amount) {
            Ok(amount) => Ok(Self {
                amounts: RareStateLog::new(EntryAmount { entry, amount }),
                states,
                index: pushed_amount,
            }),
            Err(error) => Err(AmountErr {
                values: states,
                entry,
                pushed_amount,
                _error: error,
            }),
        }
    }
    fn fallible_with_capacities(
        iter: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Result<Self, AmountErr<VecDeque<T>, Self>> {
        let mut states = VecDeque::with_capacity(states_capacity);
        states.extend(iter.into_iter());
        let pushed_amount = states.len();
        match <Self as WithAmountInternal>::usize_to_amount(pushed_amount) {
            Ok(amount) => Ok(Self {
                amounts: RareStateLog::with_capacity(
                    EntryAmount { entry, amount },
                    entries_capacity,
                ),
                states,
                index: pushed_amount,
            }),
            Err(error) => Err(AmountErr {
                values: states,
                entry,
                pushed_amount,
                _error: error,
            }),
        }
    }
    fn fallible_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<U>, AmountErr<Drain<T>, Self>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        if pushed_amount == 0 {
            self.amounts.push_present(None);
            return Ok(Some(entry));
        }
        match <Self as WithAmountInternal>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.index = self.states.len();
                self.amounts
                    .push_present(Some(EntryAmount { entry, amount }));
                Ok(None)
            }
            Err(error) => Err(AmountErr {
                values: self.states.drain(self.index..),
                entry,
                pushed_amount,
                _error: error,
            }),
        }
    }
    fn fallible_clear_with(
        &mut self,
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), AmountErr<VecDeque<T>, Self>> {
        let mut states = VecDeque::from_iter(iter.into_iter());
        let pushed_amount = states.len();
        match <Self as WithAmountInternal>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.states.clear();
                self.states.append(&mut states);
                self.amounts.clear_with(EntryAmount { entry, amount });
                self.index = pushed_amount;
                Ok(())
            }
            Err(error) => Err(AmountErr {
                values: states,
                entry,
                pushed_amount,
                _error: error,
            }),
        }
    }
}

#[doc = doc_with_amount!(impl where Infallible)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U, Err = Infallible>,
{
    pub fn new(iter: impl IntoIterator<Item = T>, entry: U) -> Self {
        // rust analyzer does not like `let Ok(ok) = result;` here
        // https://github.com/rust-lang/rust-analyzer/issues/18334
        match Self::fallible_new(iter, entry) {
            Ok(ok) => ok,
            Err(err) => match err._error {},
        }
    }
    pub fn with_capacities(
        iter: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Self {
        // rust analyzer does not like `let Ok(ok) = result;` here
        // https://github.com/rust-lang/rust-analyzer/issues/18334
        match Self::fallible_with_capacities(iter, entry, states_capacity, entries_capacity) {
            Ok(ok) => ok,
            Err(err) => match err._error {},
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) -> Option<U> {
        // rust analyzer does not like `let Ok(ok) = result;` here
        // https://github.com/rust-lang/rust-analyzer/issues/18334
        match self.fallible_push_present(c) {
            Ok(ok) => ok,
            Err(err) => match err._error {},
        }
    }
    pub fn clear_with(&mut self, iter: impl IntoIterator<Item = T>, entry: U) {
        // rust analyzer does not like `let Ok(ok) = result;` here
        // https://github.com/rust-lang/rust-analyzer/issues/18334
        match self.fallible_clear_with(iter, entry) {
            Ok(()) => (),
            Err(err) => match err._error {},
        }
    }
}

#[doc = doc_with_amount!(impl where NotUsize)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U, Amount: NotUSize>,
{
    pub fn try_new(
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<VecDeque<T>, Self>> {
        Self::fallible_new(iter, entry)
    }
    pub fn try_with_capacities(
        iter: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        entries_capacity: usize,
    ) -> Result<Self, AmountErr<VecDeque<T>, Self>> {
        Self::fallible_with_capacities(iter, entry, states_capacity, entries_capacity)
    }
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<U>, AmountErr<Drain<T>, Self>> {
        self.fallible_push_present(c)
    }
    pub fn try_clear_with(
        &mut self,
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), AmountErr<VecDeque<T>, Self>> {
        self.fallible_clear_with(iter, entry)
    }
}

#[doc = doc_with_amount!(struct)]
#[allow(private_bounds)]
impl<T, U: LoggedAt, const AMOUNT_BYTES: usize> RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<ValueEntry<Drain<T>, U>> {
        self.amounts
            .pop_past_by_logged_at(meta)
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> Drain<T> {
        let amount: usize = self
            .amounts
            .truncate_future_drain_past_by_logged_at(meta)
            .map(|entry_amount| entry_amount.amount())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU32;

    use serde::{Deserialize, Serialize};

    use crate::{
        frame::RevFrame,
        log::test::{shorten_strategy, ShortenStrategy},
    };

    use super::*;

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: RareStatesLog<char, u8>,
            #[serde(with = "crate::log::logless_state")]
            logless: RareStatesLog<char, u8>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: RareStatesLog<char, u8>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: RareStatesLog<char, u8>,
        }

        let mut original = RareStatesLog::new(['a', 'b'], 1);
        original.push_present(|mut log| {
            log.extend(['c', 'd']);
            2
        });
        original.push_present(|mut log| {
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

        let test = |log: &RareStatesLog<char, u8>, entries_len, states_len, with_capacity| {
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
        let mut original = RareStatesLog::<_, _, 1>::try_new([1, 1], 'a').unwrap();
        original
            .try_push_present(|mut log| {
                log.extend([2, 2]);
                'b'
            })
            .unwrap();
        original
            .try_push_present(|mut log| {
                log.append(&mut VecDeque::new());
                'x'
            })
            .unwrap();
        original
            .try_push_present(|mut log| {
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
            err.pushed_amount, 256,
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

    enum Push {
        States,
        Empty,
        TooMany,
    }

    impl RareStatesLog<u8, RevFrame, 1> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            max_past_len: usize, // control when the by-len strategies trigger pop/drain to align to the by-logged-at strategies
            states: Vec<u8>,
            entry: u32,
            push: Push,
            expected_entries_len: usize,
            expected_states_len: usize,
            expected_popped: Option<(Vec<u8>, u32)>,
        ) {
            let before = self.clone();
            match push {
                Push::States | Push::Empty => {
                    meta.queue_forward();
                    meta.update(|_, _| {});
                    let result = self
                        .try_push_present(|mut log| {
                            if matches!(push, Push::States) {
                                log.extend(states.clone());
                            }
                            RevFrame::checked_new(entry)
                        })
                        .ok();
                    let expected =
                        matches!(push, Push::Empty).then_some(RevFrame::checked_new(entry));
                    let after_push = self.clone();
                    assert_eq!(
                        result,
                        Some(expected),
                        "\nstrategy: {strategy:?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
                    );
                    let (actual_states, actual_entry) =
                        shorten_strategy!(self, meta, strategy, max_past_len);
                    let (expected_states, expected_entry) = expected_popped.unzip();
                    assert_eq!(
                        actual_states.unwrap_or_default(),
                        expected_states.map(|popped| Vec::from_iter(popped)).unwrap_or_default(),
                        "\nstrategy: {strategy:?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
                    );
                    if matches!(
                        strategy,
                        ShortenStrategy::PopPastByLen | ShortenStrategy::PopPastByLoggedAt
                    ) {
                        assert_eq!(
                            actual_entry, expected_entry,
                            "\nstrategy: {strategy:?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
                        );
                    }
                    assert_eq!(
                        self.entries_len(),
                        expected_entries_len,
                        "\nstrategy: {strategy:?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
                    );
                    assert_eq!(
                        self.states_len(),
                        expected_states_len,
                        "\nstrategy: {strategy:?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
                    );
                    self.test_states(before, meta, states, entry);
                }
                Push::TooMany => {
                    let frame = meta.present_world_state().wrapping_add(1);
                    let result = self.try_push_present(|mut log| {
                        log.extend([0; 256]);
                        frame
                    });
                    let result = result.map_err(
                        |AmountErr {
                             values,
                             entry,
                             pushed_amount,
                             _error: error,
                         }| AmountErr::<Vec<u8>, Self> {
                            values: Vec::from_iter(values),
                            entry,
                            pushed_amount,
                            _error: error,
                        },
                    );
                    match result {
                        Ok(_) => {
                            panic!("\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}")
                        }
                        Err(AmountErr {
                            values,
                            entry: actual_entry,
                            pushed_amount,
                            ..
                        }) => {
                            assert_eq!(
                                values, [0; 256],
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                            );
                            assert_eq!(
                                actual_entry, frame,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                            );
                            assert_eq!(
                                pushed_amount, 256,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                            );
                            assert_eq!(
                                self.entries_len(),
                                expected_entries_len,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                            );
                            assert_eq!(
                                self.states_len(),
                                expected_states_len,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                            );
                            self.test_states(before, meta, states, entry);
                        }
                    }
                }
            }
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_states: Vec<u8>,
            expected_entry: u32,
            expected_result: Result<bool, OutOfLog>,
        ) {
            let before = self.clone();
            if expected_result.is_ok() {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
            }
            let actual_result = self.forward_log();
            assert_eq!(
                actual_result, expected_result,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            self.test_states(before, meta, expected_states, expected_entry);
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_states: Vec<u8>,
            expected_entry: u32,
            expected_result: Result<bool, OutOfLog>,
        ) {
            let before = self.clone();
            if expected_result.is_ok() {
                let frame = meta.present_world_state().wrapping_sub(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
            }
            let actual_result = self.backward_log();
            assert_eq!(
                actual_result, expected_result,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            self.test_states(before, meta, expected_states, expected_entry);
        }
        fn test_states(
            &self,
            before: Self,
            meta: &RevMeta,
            expected_states: Vec<u8>,
            expected_entry: u32,
        ) {
            let (actual_states, entry) = self.get();
            let actual_states: Vec<u8> = actual_states.cloned().collect();
            assert_eq!(
                actual_states, expected_states,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            assert_eq!(
                *entry,
                RevFrame::checked_new(expected_entry),
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (Vec<u8>, u32)>,
            expected_entries_len: usize,
            expected_states_len: usize,
        ) -> Self {
            let before = self.clone();
            let mut clone = self.clone();
            let (mut states, entries) = clone.drain_future();
            let actual_future: Vec<_> = entries
                .map(|entry_amount| {
                    let states = states.by_ref().take(entry_amount.amount()).collect();
                    (states, u32::from(entry_amount.entry))
                })
                .collect();
            let expected_future: Vec<_> = expected_future
                .into_iter()
                .map(|(states, entry)| {
                    let states = Vec::from_iter(states);
                    (states, entry)
                })
                .collect();
            drop(states);
            assert_eq!(
                actual_future, expected_future,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            assert_eq!(
                clone.entries_len(),
                expected_entries_len,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            assert_eq!(
                clone.states_len(),
                expected_states_len,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut log = RareStatesLog::try_new(vec![0; 0], meta.present_world_state()).unwrap();

            log.test_forward(meta, strategy, 0, vec![0; 0], 0, Push::Empty, 0, 0, None);
            log.test_forward(meta, strategy, 1, vec![2; 2], 2, Push::States, 1, 2, None);
            // does not pop yet because the skip after the initial state is still in log range
            log.test_forward(meta, strategy, 2, vec![3; 3], 3, Push::States, 2, 5, None);
            // does not pop yet because while the skip after the initial state is no longer in log range,
            // multiple uses of `push_present` by the user at the same frame makes the skips as frame offset
            // an unreliable indicator to pop here
            log.test_forward(meta, strategy, 3, vec![3; 3], 3, Push::Empty, 2, 5, None);
            // pops oldest entry as the second-oldest entry is also out of log now
            log.test_forward(
                meta,
                strategy,
                3,
                vec![5; 5],
                5,
                Push::States,
                2,
                10,
                Some((vec![0; 0], 0)),
            );

            meta.set_oldest_frame(1); // make log start accessible again to test out-of-log

            log.test_backward_log(meta, vec![3; 3], 3, Ok(true));
            log.test_backward_log(meta, vec![3; 3], 3, Ok(false));
            log.test_backward_log(meta, vec![2; 2], 2, Ok(true));
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, vec![2; 2], 2, Err(OutOfLog));

            log.test_forward_log(meta, vec![3; 3], 3, Ok(true));
            log.test_forward_log(meta, vec![3; 3], 3, Ok(false));
            log.test_forward_log(meta, vec![5; 5], 5, Ok(true));
            // out of log, no mutations happend to both meta and log here
            log.test_forward_log(meta, vec![5; 5], 5, Err(OutOfLog));

            log.test_backward_log(meta, vec![3; 3], 3, Ok(true));
            log.test_backward_log(meta, vec![3; 3], 3, Ok(false));
            log.test_backward_log(meta, vec![2; 2], 2, Ok(true));

            let log_clone = log.test_drain_future([(vec![3; 3], 3), (vec![5; 5], 5)], 0, 2);

            for mut log in [log, log_clone] {
                // all entries are truncated as they are in the future
                log.test_forward(meta, strategy, 3, vec![4; 4], 3, Push::States, 1, 6, None);

                // storing too many states fails
                log.test_forward(meta, strategy, 3, vec![4; 4], 3, Push::TooMany, 1, 6, None);
            }
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareStatesLog<usize, RevFrame, 1>>();
    }
}
