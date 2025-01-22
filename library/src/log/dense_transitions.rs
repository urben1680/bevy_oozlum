use std::{
    collections::{
        vec_deque::{Drain, IterMut},
        TryReserveError, VecDeque,
    },
    fmt::Debug,
};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

use super::{
    doc_with_amount, AmountErr, DenseTransitionLog, EntryAmount, LogMut, OutOfLog, ValueEntry,
    USIZE_BYTES,
};

#[doc = doc_with_amount!(struct)]
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DenseTransitionsLog<T, U = (), const AMOUNT_BYTES: usize = USIZE_BYTES> {
    amounts: DenseTransitionLog<EntryAmount<U, AMOUNT_BYTES>>,
    transitions: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::{DenseTransitionLog, DenseTransitionsLog, EntryAmount};

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for DenseTransitionsLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (
            <DenseTransitionLog<EntryAmount<U, AMOUNT_BYTES>> as WithCapacity>::Se<'se>,
            WithCapacityWrapper<&'se VecDeque<T>>,
            usize,
        );
        type De = (
            <DenseTransitionLog<EntryAmount<U, AMOUNT_BYTES>> as WithCapacity>::De,
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
        fn from_with_capacity(
            (amounts, WithCapacityWrapper(transitions), index): Self::De,
        ) -> Self {
            Self {
                amounts: DenseTransitionLog::from_with_capacity(amounts),
                transitions,
                index,
            }
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity
        for DenseTransitionsLog<T, U, AMOUNT_BYTES>
    {
        type Se<'se>
            = (usize, usize)
        where
            T: 'se,
            U: 'se;
        type De = (usize, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (self.entries_capacity(), self.transitions_capacity())
        }
        fn from_logless_with_capacity((entries_capacity, transitions_capacity): Self::De) -> Self {
            Self::with_capacities(entries_capacity, transitions_capacity)
        }
    }
}

#[doc = doc_with_amount!(impl)]
impl<T, U, const AMOUNT_BYTES: usize> Default for DenseTransitionsLog<T, U, AMOUNT_BYTES> {
    fn default() -> Self {
        Self::new()
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> DenseTransitionsLog<T, U, AMOUNT_BYTES> {
    pub const fn new() -> Self {
        Self {
            amounts: DenseTransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(entries_capacity: usize, transitions_capacity: usize) -> Self {
        Self {
            amounts: DenseTransitionLog::with_capacity(entries_capacity),
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
    pub fn drain_future(&mut self) -> (Drain<T>, Drain<EntryAmount<U, AMOUNT_BYTES>>) {
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
    pub fn backward_log(&mut self) -> Result<ValueEntry<IterMut<T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.backward_log()?;
        self.index -= entry_amount.amount();
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
    }
    pub fn forward_log(&mut self) -> Result<ValueEntry<IterMut<T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.forward_log()?;
        self.index += entry_amount.amount();
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
    }
}

impl<T, U> DenseTransitionsLog<T, U, USIZE_BYTES> {
    pub fn push<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        self.amounts.push(entry_amount);
        self.index = self.transitions.len();
    }
    pub fn push_and_pop_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Option<ValueEntry<Drain<T>, U>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        self.index = self.transitions.len();
        self.amounts
            .push_and_pop_past(max_past_len, entry_amount)
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                self.index -= amount;
                ValueEntry {
                    value: self.transitions.drain(..amount),
                    entry: entry_amount.entry,
                }
            })
    }
    pub fn push_and_drain_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> (Drain<T>, Drain<EntryAmount<U, USIZE_BYTES>>) {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        self.index = self.transitions.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, entry_amount);
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain.map(|entry_amount| entry_amount.amount()).sum();
        self.index -= amount;
        (
            self.transitions.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        )
    }
}

impl<T, U, const AMOUNT_BYTES: usize> DenseTransitionsLog<T, U, AMOUNT_BYTES> {
    pub fn try_push<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<Drain<T>, U, AMOUNT_BYTES>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            let values = self.transitions.drain(self.index..);
            return Err(AmountErr {
                values,
                entry_amount,
            });
        }
        self.amounts.push(entry_amount);
        self.index = self.transitions.len();
        Ok(())
    }
    pub fn try_push_and_pop_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<ValueEntry<Drain<T>, U>>, AmountErr<Drain<T>, U, AMOUNT_BYTES>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            let values = self.transitions.drain(self.index..);
            return Err(AmountErr {
                values,
                entry_amount,
            });
        }
        self.index = self.transitions.len();
        Ok(self
            .amounts
            .push_and_pop_past(max_past_len, entry_amount)
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                self.index -= amount;
                ValueEntry {
                    value: self.transitions.drain(..amount),
                    entry: entry_amount.entry,
                }
            }))
    }
    pub fn try_push_and_drain_past<Out: Into<U>>(
        &mut self,
        max_past_len: usize,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(Drain<T>, Drain<EntryAmount<U, AMOUNT_BYTES>>), AmountErr<Drain<T>, U, AMOUNT_BYTES>>
    {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount::new(entry, pushed_amount);
        if pushed_amount != entry_amount.amount() {
            let values = self.transitions.drain(self.index..);
            return Err(AmountErr {
                values,
                entry_amount,
            });
        }
        self.index = self.transitions.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, entry_amount);
        let to_drain_len = to_drain.len();
        let amount: usize = to_drain.map(|entry_amount| entry_amount.amount()).sum();
        self.index -= amount;
        Ok((
            self.transitions.drain(..amount),
            self.amounts.drain_past(to_drain_len),
        ))
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU32;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::{
        frame::RevFrame,
        log::test::{shorten_strategy, ShortenStrategy},
        meta::RevMeta,
    };

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: DenseTransitionsLog<char, u8>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: DenseTransitionsLog<char, u8>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: DenseTransitionsLog<char, u8>,
        }

        let mut original = DenseTransitionsLog::new();
        original.push(|mut log| {
            log.extend(['a', 'b']);
            1
        });
        original.push(|mut log| {
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
            |log: &DenseTransitionsLog<char, u8>, entries_len, transitions_len, with_capacity| {
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
    impl DenseTransitionsLog<u8, RevFrame, 1> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            push: Vec<u8>,
            expected_entries_len: usize,
            expected_transitions_len: usize,
            expected_popped: Option<(Vec<u8>, u32)>,
        ) {
            let before = self.clone();
            if push.len() < u8::MAX as usize {
                meta.queue_forward();
                meta.update(|_, _| {});
                let result = self.try_push(|mut log| {
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
                let (actual_states, actual_entry) =
                    shorten_strategy!(self, meta, strategy, meta.past_world_states());
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
            } else {
                let result = self.try_push(|mut log| {
                    log.extend(push.clone());
                    meta.present_world_state().wrapping_add(1)
                });
                let result = result.map_err(
                    |AmountErrOld {
                         values,
                         entry,
                         pushed_amount,
                         _error: error,
                     }| AmountErrOld::<Vec<u8>, Self> {
                        values: Vec::from_iter(values),
                        entry,
                        pushed_amount,
                        _error: error,
                    },
                );
                match result {
                    Ok(()) => {
                        panic!("\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}")
                    }
                    Err(AmountErrOld {
                        values,
                        pushed_amount,
                        ..
                    }) => {
                        assert_eq!(
                            values, push,
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
                            self.transitions_len(),
                            expected_transitions_len,
                            "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                        );
                    }
                }
            }
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transitions: Result<Vec<u8>, OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transitions = expected_transitions.map(|transitions| {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
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
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            )
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transitions: Result<Vec<u8>, OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transitions = expected_transitions.map(|transitions| {
                let frame = meta.present_world_state();
                meta.queue_log(frame.wrapping_sub(1)).unwrap();
                meta.update(|_, _| {});
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
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            )
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (Vec<u8>, u32)>,
            expected_entries_len: usize,
            expected_transitions_len: usize,
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
                clone.transitions_len(),
                expected_transitions_len,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut log = DenseTransitionsLog::new();

            log.test_forward(meta, strategy, vec![1; 1], 1, 1, None);
            log.test_forward(meta, strategy, vec![2; 2], 2, 3, None);
            // shortened log
            log.test_forward(meta, strategy, vec![3; 3], 2, 5, Some((vec![1; 1], 1)));

            log.test_backward_log(meta, Ok(vec![3; 3]));
            log.test_backward_log(meta, Ok(vec![2; 2]));
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, Err(OutOfLog));

            log.test_forward_log(meta, Ok(vec![2; 2]));
            log.test_forward_log(meta, Ok(vec![3; 3]));
            // out of log, no mutations happend to both meta and log here
            log.test_forward_log(meta, Err(OutOfLog));

            log.test_backward_log(meta, Ok(vec![3; 3]));
            log.test_backward_log(meta, Ok(vec![2; 2]));

            let clone = log.test_drain_future([(vec![2; 2], 2), (vec![3; 3], 3)], 0, 0);

            for mut log in [log, clone] {
                // all entries are truncated as they are in the future
                log.test_forward(meta, strategy, vec![4; 4], 1, 4, None);

                // storing too many transitions fails
                log.test_forward(meta, strategy, vec![0; 256], 1, 4, None);
            }
        }
    }
    */

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<DenseTransitionsLog<usize, RevFrame, 1>>();
    }
}
