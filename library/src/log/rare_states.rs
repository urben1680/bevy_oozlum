use std::{
    collections::{TryReserveError, VecDeque},
    convert::Infallible,
    fmt::Debug,
    ops::{Deref, Range},
};

use bevy::{reflect::Reflect, utils::default};

use crate::meta::RevMeta;

use super::{
    doc_with_amount, impl_with_amount, AmountErr, EntryAmount, LogIter, LogMut, LoggedAt, NotUSize,
    OutOfLog, RareStateLog, ValueEntry, WithAmountInternal,
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
            let amounts = self.amounts.get_logless_state();
            let range = self.get_range_entry().0;
            (
                amounts,
                WithRange {
                    deque: &self.states,
                    range,
                },
            )
        }
        fn from_logless_state((amounts, states): Self::De) -> Result<Self, String> {
            <RareStateLog<EntryAmount<Self>> as LoglessState>::from_logless_state(amounts).map(
                |amounts| {
                    let index = amounts.amount();
                    Self {
                        amounts,
                        states,
                        index,
                    }
                },
            )
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
        fn from_with_capacity(
            (amounts, WithCapacityWrapper(states), index): Self::De,
        ) -> Result<Self, String> {
            WithCapacity::from_with_capacity(amounts).map(|amounts| Self {
                amounts,
                states,
                index,
            })
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
            let amounts_se = self.amounts.get_logless_with_capacity();
            let range = self.get_range_entry().0;
            (
                amounts_se,
                WithCapacityWrapper(WithRange {
                    deque: &self.states,
                    range,
                }),
            )
        }
        fn from_logless_with_capacity(
            (amounts, WithCapacityWrapper(states)): Self::De,
        ) -> Result<Self, String> {
            RareStateLog::from_logless_with_capacity(amounts).map(|amounts| {
                let index = amounts.amount();
                Self {
                    amounts,
                    states,
                    index,
                }
            })
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
        let entry_amount = &self.amounts;
        let amount = entry_amount.amount();
        let from = self.index - amount;
        (from..self.index, &entry_amount.entry)
    }
    pub fn get(&self) -> (impl LogIter<&T>, &U) {
        let (range, entry) = self.get_range_entry();
        let states = self.states.range(range);
        (states, entry)
    }
    pub fn past_end(&self) -> Option<(impl LogIter<&T>, &U)> {
        let entry_amount = self.amounts.past_end()?;
        let to = entry_amount.amount();
        let states = self.states.range(..to);
        Some((states, &entry_amount.entry))
    }
    pub fn pop_past(&mut self) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<EntryAmount<Self>>) {
        (self.states.drain(self.index..), self.amounts.drain_future())
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        let amount = self.amounts.amount;
        let amount = <Self as WithAmountInternal>::amount_to_usize(amount);
        self.states.drain(..self.index);
        self.states.truncate(amount);
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
        self.index += self.amounts.amount();
        Ok(())
    }
    pub fn pop_past_by_len(
        &mut self,
        max_past_len: usize,
    ) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past_by_len(max_past_len)
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_len(max_past_len)
            .map(|entry_amount| entry_amount.amount())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
    }
    fn drain_past_by_amount(
        &mut self,
        entry_amount: EntryAmount<Self>,
    ) -> ValueEntry<impl LogIter<T>, U> {
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
    ) -> Result<Option<U>, AmountErr<impl LogIter<T>, Self>> {
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
            Err(error) => {
                let states = self.states.drain(self.index..);
                Err(AmountErr {
                    values: states,
                    entry,
                    pushed_amount,
                    _error: error,
                })
            }
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
    ) -> Result<Option<U>, AmountErr<impl LogIter<T>, Self>> {
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
    pub fn pop_past_by_logged_at(
        &mut self,
        meta: &RevMeta,
    ) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past_by_logged_at(meta)
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> impl LogIter<T> {
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
    use serde::{Deserialize, Serialize};

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
}
