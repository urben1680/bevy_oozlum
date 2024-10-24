use std::{
    collections::{TryReserveError, VecDeque},
    convert::Infallible,
    fmt::Debug,
};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

use crate::meta::RevMeta;

use super::{
    doc_with_amount, impl_with_amount, AmountErr, EntryAmount, LogIter, LogMut, LoggedAt, NotUSize,
    OutOfLog, TransitionLog, ValueEntry, WithAmountInternal,
};

#[doc = doc_with_amount!(struct)]
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionsLog<T, U = (), const AMOUNT_BYTES: usize = 0>
where
    Self: WithAmountInternal<Entry = U>,
{
    amounts: TransitionLog<EntryAmount<Self>>,
    transitions: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::{EntryAmount, TransitionLog, TransitionsLog, WithAmountInternal};

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for TransitionsLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmountInternal<Entry = U>,
    {
        type Se<'se> = (
            <TransitionLog<EntryAmount<Self>> as WithCapacity>::Se<'se>,
            WithCapacityWrapper<&'se VecDeque<T>>,
            usize,
        );
        type De = (
            <TransitionLog<EntryAmount<Self>> as WithCapacity>::De,
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
            TransitionLog::from_with_capacity(with_capacity.0).map(|amounts| Self {
                amounts,
                transitions: with_capacity.1 .0,
                index: with_capacity.2,
            })
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity for TransitionsLog<T, U, AMOUNT_BYTES>
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
            Ok(Self::with_capacities(entries_capacity, transitions_capacity))
        }
    }
}

impl_with_amount!(TransitionsLog);

#[doc = doc_with_amount!(impl)]
impl<T, U, const AMOUNT_BYTES: usize> Default for TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    fn default() -> Self {
        Self::new()
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U>,
{
    pub const fn new() -> Self {
        Self {
            amounts: TransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(entries_capacity: usize, transitions_capacity: usize) -> Self {
        Self {
            amounts: TransitionLog::with_capacity(entries_capacity),
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
        }
    }
    pub fn entries_len(&self) -> usize {
        self.amounts.len()
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn entries_capacity(&self) -> usize {
        self.amounts.capacity()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn entries_is_empty(&self) -> bool {
        self.amounts.is_empty()
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn entries_reserve(&mut self, additional: usize) {
        self.amounts.reserve(additional)
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn entries_reserve_exact(&mut self, additional: usize) {
        self.amounts.reserve_exact(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn entries_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn entries_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve_exact(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn entries_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn entries_shrink_to_fit(&mut self) {
        self.amounts.shrink_to_fit()
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
        self.transitions.clear();
        self.amounts.clear();
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<ValueEntry<impl LogIter<&mut T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.backward_log()?;
        self.index -= entry_amount.amount();
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
    }
    pub fn forward_log(&mut self) -> Result<ValueEntry<impl LogIter<&mut T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.forward_log()?;
        self.index += entry_amount.amount();
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
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
    ) -> Result<(), AmountErr<impl LogIter<T>, Self>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        match <Self as WithAmountInternal>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.index = self.transitions.len();
                self.amounts.push_present(EntryAmount { entry, amount });
                Ok(())
            }
            Err(error) => {
                let transitions = self.transitions.drain(self.index..);
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
impl<T, U, const AMOUNT_BYTES: usize> TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U, Err = Infallible>,
{
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        // rust analyzer does not like `let Ok(ok) = result;` here
        // https://github.com/rust-lang/rust-analyzer/issues/18334
        match self.fallible_push_present(c) {
            Ok(()) => (),
            Err(err) => match err._error {},
        }
    }
}

#[doc = doc_with_amount!(impl where NotUsize)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmountInternal<Entry = U, Amount: NotUSize>,
{
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<impl LogIter<T>, Self>> {
        self.fallible_push_present(c)
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U: LoggedAt, const AMOUNT_BYTES: usize> TransitionsLog<T, U, AMOUNT_BYTES>
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
    use std::num::NonZeroUsize;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::{
        log::test::{shorten_strategy, ShortenStrategy},
        meta::RevMeta,
        RevFrame,
    };

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: TransitionsLog<char, u8>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: TransitionsLog<char, u8>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: TransitionsLog<char, u8>,
        }

        let mut original = TransitionsLog::new();
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

        let test = |log: &TransitionsLog<char, u8>, entries_len, transitions_len, with_capacity| {
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

    impl TransitionsLog<u8, RevFrame, 1> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            push: Result<[u8; 2], [u8; 256]>,
            expected_entries_len: usize,
            expected_transitions_len: usize,
            expected_popped: Option<([u8; 2], usize)>,
        ) {
            let before = self.clone();
            match push {
                Ok(push) => {
                    meta.queue_forward();
                    meta.update();
                    let result = self.try_push_present(|mut log| {
                        log.extend(push.clone());
                        meta.present_world_state()
                    });
                    let is_ok = result.is_ok();
                    drop(result);
                    let after_push = self.clone();
                    assert!(
                        is_ok,
                        "\nstrategy: {strategy:?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
                    );
                    let (actual_states, actual_entry) = shorten_strategy!(self, meta, strategy);
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
                        self.transitions_len(),
                        expected_transitions_len,
                        "\nstrategy: {strategy:?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
                    );
                }
                Err(push) => {
                    let result = self.try_push_present(|mut log| {
                        log.extend(push.clone());
                        meta.present_world_state().wrapping_add(1)
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
                        Ok(()) => {
                            panic!("\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {self:#?}")
                        }
                        Err(AmountErr {
                            values,
                            pushed_amount,
                            ..
                        }) => {
                            assert_eq!(
                                values, push,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {self:#?}",
                            );
                            assert_eq!(
                                pushed_amount, 256,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {self:#?}",
                            );
                            assert_eq!(
                                self.entries_len(),
                                expected_entries_len,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {self:#?}",
                            );
                            assert_eq!(
                                self.transitions_len(),
                                expected_transitions_len,
                                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {self:#?}",
                            );
                        }
                    }
                }
            }
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transitions: Result<[u8; 2], OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transitions = expected_transitions.map(|transitions| {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update();
                (Vec::from_iter(transitions), frame)
            });
            let actual_transitions = self.forward_log().map(|value_entry| {
                (
                    value_entry
                        .value
                        .map(|transition| *transition)
                        .collect::<Vec<_>>(),
                    *value_entry.entry,
                )
            });
            assert_eq!(
                actual_transitions, expected_transitions,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_forward: {self:#?}",
            )
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transitions: Result<[u8; 2], OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transitions = expected_transitions.map(|transitions| {
                let frame = meta.present_world_state();
                meta.queue_log(frame.wrapping_sub(1)).unwrap();
                meta.update();
                (Vec::from_iter(transitions), frame)
            });
            let actual_transitions = self.backward_log().map(|value_entry| {
                (
                    value_entry
                        .value
                        .map(|transition| *transition)
                        .collect::<Vec<_>>(),
                    *value_entry.entry,
                )
            });
            assert_eq!(
                actual_transitions, expected_transitions,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_backward: {self:#?}",
            )
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = ([u8; 2], usize)>,
            expected_entries_len: usize,
            expected_transitions_len: usize,
        ) -> Self {
            let before = self.clone();
            let mut clone = self.clone();
            let (mut states, entries) = clone.drain_future();
            let actual_future: Vec<_> = entries
                .map(|entry_amount| {
                    let states = states.by_ref().take(entry_amount.amount()).collect();
                    (states, usize::from(entry_amount.entry))
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
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            assert_eq!(
                clone.entries_len(),
                expected_entries_len,
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            assert_eq!(
                clone.transitions_len(),
                expected_transitions_len,
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroUsize::new(3), 0, false);
            let mut log = TransitionsLog::new();

            log.test_forward(meta, strategy, Ok([1, 1]), 1, 2, None);
            log.test_forward(meta, strategy, Ok([2, 2]), 2, 4, None);
            // shortened log
            log.test_forward(meta, strategy, Ok([3, 3]), 2, 4, Some(([1, 1], 1)));

            log.test_backward_log(meta, Ok([3, 3]));
            log.test_backward_log(meta, Ok([2, 2]));
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, Err(OutOfLog));

            log.test_forward_log(meta, Ok([2, 2]));
            log.test_forward_log(meta, Ok([3, 3]));
            // out of log, no mutations happend to both meta and log here
            log.test_forward_log(meta, Err(OutOfLog));

            log.test_backward_log(meta, Ok([3, 3]));
            log.test_backward_log(meta, Ok([2, 2]));

            let clone = log.test_drain_future([([2, 2], 2), ([3, 3], 3)], 0, 0);

            for mut log in [log, clone] {
                // all entries are truncated as they are in the future
                log.test_forward(meta, strategy, Ok([4, 4]), 1, 2, None);

                // storing too many transitions fails
                log.test_forward(meta, strategy, Err([0; 256]), 1, 2, None);
            }
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<TransitionsLog<usize, RevFrame, 1>>();
    }
}
