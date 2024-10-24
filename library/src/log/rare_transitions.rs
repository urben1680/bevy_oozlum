use std::{
    collections::{TryReserveError, VecDeque},
    convert::Infallible,
    fmt::Debug,
};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

use crate::meta::RevMeta;

use super::{
    doc_with_amount, impl_with_amount, AmountErr, EntryAmount, LogIter, LogMut, LoggedAt, NotUSize,
    OutOfLog, RareTransitionLog, ValueEntry, WithAmountInternal,
};

#[doc = doc_with_amount!(struct)]
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareTransitionsLog<T, U = (), const AMOUNT_BYTES: usize = 0>
where
    Self: WithAmountInternal<Entry = U>,
{
    amounts: RareTransitionLog<EntryAmount<Self>>,
    transitions: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::{EntryAmount, RareTransitionLog, RareTransitionsLog, WithAmountInternal};

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for RareTransitionsLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmountInternal<Entry = U>,
    {
        type Se<'se> = (
            <RareTransitionLog<EntryAmount<Self>> as WithCapacity>::Se<'se>,
            WithCapacityWrapper<&'se VecDeque<T>>,
            usize,
        );
        type De = (
            <RareTransitionLog<EntryAmount<Self>> as WithCapacity>::De,
            WithCapacityWrapper<VecDeque<T>>,
            usize,
        );
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                self.amounts.get_with_capacity(),
                WithCapacityWrapper(&self.transitions),
                self.index,
            )
        }
        fn from_with_capacity(with_capacity: Self::De) -> Result<Self, String> {
            RareTransitionLog::from_with_capacity(with_capacity.0).map(|amounts| Self {
                amounts,
                transitions: with_capacity.1 .0,
                index: with_capacity.2,
            })
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity for RareTransitionsLog<T, U, AMOUNT_BYTES>
    where
        Self: WithAmountInternal<Entry = U>,
    {
        type Se<'se> = (usize, usize) where T: 'se, U: 'se;
        type De = (usize, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (self.entries_capacity(), self.transitions_capacity())
        }
        fn from_logless_with_capacity(
            (entries_capacity, transitions_capacity): Self::De,
        ) -> Result<Self, String> {
            Ok(Self::with_capacities(
                entries_capacity,
                transitions_capacity,
            ))
        }
    }
}

impl_with_amount!(RareTransitionsLog);

#[doc = doc_with_amount!(impl)]
impl<T, U, const AMOUNT_BYTES: usize> Default for RareTransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    fn default() -> Self {
        Self::new()
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> RareTransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    pub const fn new() -> Self {
        Self {
            amounts: RareTransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(entries_capacity: usize, transitions_capacity: usize) -> Self {
        Self {
            amounts: RareTransitionLog::with_capacity(entries_capacity),
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
    pub fn past_end(&self) -> Option<&U> {
        self.amounts
            .past_end()
            .map(|entry_amount| &entry_amount.entry)
    }
    pub fn pop_past(&mut self) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<EntryAmount<Self>>) {
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
    pub fn backward_log(
        &mut self,
    ) -> Result<Option<ValueEntry<impl LogIter<&mut T>, &mut U>>, OutOfLog> {
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
    pub fn forward_log(
        &mut self,
    ) -> Result<Option<ValueEntry<impl LogIter<&mut T>, &mut U>>, OutOfLog> {
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
        self.transitions.drain(..amount)
    }
    fn drain_past_by_amount(
        &mut self,
        entry_amount: EntryAmount<Self>,
    ) -> ValueEntry<impl LogIter<T>, U> {
        let amount = entry_amount.amount();
        self.index -= amount;
        ValueEntry {
            value: self.transitions.drain(..amount),
            entry: entry_amount.entry,
        }
    }
    fn fallible_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<U>, AmountErr<impl LogIter<T>, Self>> {
        self.transitions.truncate(self.index);
        let previous_len = self.transitions.len();
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - previous_len;
        if pushed_amount == 0 {
            self.amounts.push_present(None);
            return Ok(Some(entry));
        }
        match <Self as WithAmountInternal>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.amounts
                    .push_present(Some(EntryAmount { entry, amount }));
                self.index += pushed_amount;
                Ok(None)
            }
            Err(error) => {
                let transitions = self.transitions.drain(previous_len..);
                Err(AmountErr {
                    values: transitions,
                    entry,
                    pushed_amount,
                    _error: error,
                })
            }
        }
    }
}

#[doc = doc_with_amount!(impl where Infallible)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> RareTransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U, Err = Infallible>,
{
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) -> Option<U> {
        // rust analyzer does not like `let Ok(ok) = result;` here
        // https://github.com/rust-lang/rust-analyzer/issues/18334
        match self.fallible_push_present(c) {
            Ok(ok) => ok,
            Err(err) => match err._error {},
        }
    }
}

#[doc = doc_with_amount!(impl where NotUsize)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> RareTransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U, Amount: NotUSize>,
{
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<U>, AmountErr<impl LogIter<T>, Self>> {
        self.fallible_push_present(c)
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U: LoggedAt, const AMOUNT_BYTES: usize> RareTransitionsLog<T, U, AMOUNT_BYTES>
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
        self.transitions.drain(..amount)
    }
}

#[cfg(test)]
mod test {
    use std::{borrow::Borrow, num::NonZeroUsize};

    use serde::{Deserialize, Serialize};

    use crate::log::PackedRevFrame;

    use super::*;

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: RareTransitionsLog<char, u8>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: RareTransitionsLog<char, u8>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: RareTransitionsLog<char, u8>,
        }

        let mut original = RareTransitionsLog::new();
        original.push_present(|mut log| {
            log.extend(['a', 'b']);
            1
        });
        original.push_present(|mut log| {
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

        let test =
            |log: &RareTransitionsLog<char, u8>, entries_len, transitions_len, with_capacity| {
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

    /*
    #[test]
    fn test() {
        let mut meta_and_logs = MetaAndLogs::new(NonZeroUsize::new(3));

        meta_and_logs.forward(Ok([]), 1, 0);
        meta_and_logs.forward(Ok([1]), 2, 1);
        // pop_front called internally
        meta_and_logs.forward(Ok([2, 3]), 2, 3);
        meta_and_logs.forward(Ok([]), 2, 2);
        meta_and_logs.forward(Ok([4, 5, 6, 7]), 2, 4);

        meta_and_logs.backward_log(Ok(Some([4, 5, 6, 7])));
        meta_and_logs.backward_log::<0>(Ok(None));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log::<0>(Err(OutOfLog));

        meta_and_logs.forward_log::<0>(Ok(None));
        meta_and_logs.forward_log(Ok(Some([4, 5, 6, 7])));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log::<0>(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(Some([4, 5, 6, 7])));
        meta_and_logs.backward_log::<0>(Ok(None));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(Ok([]), 1, 0);

        // amount of transitions is stored as u8, cannot store more than 255 transitions per push
        meta_and_logs.forward(Err([11; 256]), 1, 0);
    }
    */
    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty()
            .register::<RareTransitionsLog<usize, PackedRevFrame, 1>>();
    }
}
