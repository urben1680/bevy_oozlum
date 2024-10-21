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
            (self.log_capacity(), self.transitions_capacity())
        }
        fn from_logless_with_capacity(
            (log_capacity, transitions_capacity): Self::De,
        ) -> Result<Self, String> {
            Ok(Self::with_capacities(log_capacity, transitions_capacity))
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
    pub fn with_capacities(log_capacity: usize, transitions_capacity: usize) -> Self {
        Self {
            amounts: RareTransitionLog::with_capacity(log_capacity),
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.amounts.log_len()
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn log_capacity(&self) -> usize {
        self.amounts.transitions_capacity()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.amounts.log_is_empty()
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn log_reserve(&mut self, additional: usize) {
        self.amounts.transitions_reserve(additional)
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn log_reserve_exact(&mut self, additional: usize) {
        self.amounts.transitions_reserve_exact(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn log_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.transitions_try_reserve(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn log_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.transitions_try_reserve_exact(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn log_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.transitions_shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn log_shrink_to_fit(&mut self) {
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

    use crate::log::PackedRevFrame;

    use super::*;
    /*
        use crate::{log::GetLoggedAt, meta::RevMeta};

        #[derive(Clone, Debug)]
        struct MetaAndLogs {
            meta: RevMeta,
            with_timestamp: [RareTransitionsLog<usize, GetLoggedAt, 1>; 2],
            one_per_frame: [RareTransitionsLog<usize, (), 1>; 2],
        }

        fn collect(iter: impl IntoIterator<Item: Borrow<usize>>) -> Vec<usize> {
            iter.into_iter().map(|val| val.borrow().clone()).collect()
        }

        impl MetaAndLogs {
            fn new(max_len: Option<NonZeroUsize>) -> Self {
                Self {
                    meta: RevMeta::new(max_len, 0, false),
                    with_timestamp: Default::default(),
                    one_per_frame: Default::default(),
                }
            }
            fn forward<const N: usize>(
                &mut self,
                transitions: Result<[usize; N], [usize; N]>,
                minimum_log_len: usize,
                expected_transitions_len: usize,
            ) {
                let previous = self.clone();

                self.meta.queue_forward();
                self.meta.update();

                let (transitions, expected_ok) = match transitions {
                    Ok(transitions) => (transitions, true),
                    Err(transitions) => (transitions, false),
                };

                self.with_timestamp[0].pop_past_by_timestamp(*self.meta.log_range().start());
                let middle = self.with_timestamp[0].clone();
                let is_ok = self.with_timestamp[0]
                    .try_push_present(|mut log| {
                        log.extend(transitions);
                        self.meta.with_logged_at(())
                    })
                    .is_ok();
                assert_eq!(
                    is_ok, expected_ok,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                );
                assert!(
                    self.with_timestamp[0].log_len() >= minimum_log_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[0],
                    self.with_timestamp[0]
                );
                assert_eq!(
                    self.with_timestamp[0].transitions_len(),
                    expected_transitions_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[0],
                    self.with_timestamp[0]
                );

                let _ = self.with_timestamp[1].drain_past_by_timestamp(*self.meta.log_range().start());
                let middle = self.with_timestamp[1].clone();
                let is_ok = self.with_timestamp[1]
                    .try_push_present(|mut log| {
                        log.extend(transitions);
                        self.meta.with_logged_at(())
                    })
                    .is_ok();
                assert_eq!(
                    is_ok, expected_ok,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                );
                assert!(
                    self.with_timestamp[1].log_len() >= minimum_log_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[1],
                    self.with_timestamp[1]
                );
                assert_eq!(
                    self.with_timestamp[1].transitions_len(),
                    expected_transitions_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[1],
                    self.with_timestamp[1]
                );

                let is_ok = self.one_per_frame[0]
                    .try_push_present(|mut log| {
                        log.extend(transitions);
                    })
                    .is_ok();
                let middle = self.one_per_frame[0].clone();
                self.one_per_frame[0].pop_past_by_len(self.meta.past_world_states());
                assert_eq!(
                    is_ok, expected_ok,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                );
                assert!(
                    self.one_per_frame[0].log_len() >= minimum_log_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[0],
                    self.one_per_frame[0]
                );
                assert_eq!(
                    self.one_per_frame[0].transitions_len(),
                    expected_transitions_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[0],
                    self.one_per_frame[0]
                );

                let is_ok = self.one_per_frame[1]
                    .try_push_present(|mut log| {
                        log.extend(transitions);
                    })
                    .is_ok();
                let middle = self.one_per_frame[1].clone();
                let _ = self.one_per_frame[1].drain_past_by_len(self.meta.past_world_states());
                assert_eq!(
                    is_ok, expected_ok,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                );
                assert!(
                    self.one_per_frame[1].log_len() >= minimum_log_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[1],
                    self.one_per_frame[1]
                );
                assert_eq!(
                    self.one_per_frame[1].transitions_len(),
                    expected_transitions_len,
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[1],
                    self.one_per_frame[1]
                );
            }
            fn backward_log<const N: usize>(
                &mut self,
                expected_transitions: Result<Option<[usize; N]>, OutOfLog>,
            ) {
                let previous = self.clone();
                match expected_transitions {
                    Ok(expected_transitions) => {
                        assert!(
                            self.meta
                                .queue_log(self.meta.present_world_state() - 1)
                                .is_ok(),
                            "\npreviously: {previous:?}\nnow: {self:?}"
                        );
                        self.meta.update();

                        let expected_transitions =
                            Ok(expected_transitions.map(|transitions| collect(transitions)));

                        let transitions = self.with_timestamp[0]
                            .backward_log()
                            .map(|ok| ok.map(|with_timestamp| collect(with_timestamp.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                        );

                        let transitions = self.with_timestamp[1]
                            .backward_log()
                            .map(|ok| ok.map(|with_timestamp| collect(with_timestamp.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                        );

                        let transitions = self.one_per_frame[0]
                            .backward_log()
                            .map(|ok| ok.map(|value| collect(value.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                        );

                        let transitions = self.one_per_frame[1]
                            .backward_log()
                            .map(|ok| ok.map(|value| collect(value.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                        );
                    }
                    Err(OutOfLog) => {
                        assert!(
                            self.with_timestamp[0].backward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.with_timestamp[0],
                            self.with_timestamp[0]
                        );
                        assert!(
                            self.with_timestamp[1].backward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.with_timestamp[1],
                            self.with_timestamp[1]
                        );
                        assert!(
                            self.one_per_frame[0].backward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.one_per_frame[0],
                            self.one_per_frame[0]
                        );
                        assert!(
                            self.one_per_frame[1].backward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.one_per_frame[1],
                            self.one_per_frame[1]
                        );
                    }
                }
            }
            fn forward_log<const N: usize>(
                &mut self,
                expected_transitions: Result<Option<[usize; N]>, OutOfLog>,
            ) {
                let previous = self.clone();
                match expected_transitions {
                    Ok(expected_transitions) => {
                        assert!(
                            self.meta
                                .queue_log(self.meta.present_world_state() + 1)
                                .is_ok(),
                            "\npreviously: {previous:?}\nnow: {self:?}"
                        );
                        self.meta.update();

                        let expected_transitions =
                            Ok(expected_transitions.map(|transitions| collect(transitions)));

                        let transitions = self.with_timestamp[0]
                            .forward_log()
                            .map(|ok| ok.map(|with_timestamp| collect(with_timestamp.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                        );

                        let transitions = self.with_timestamp[1]
                            .forward_log()
                            .map(|ok| ok.map(|with_timestamp| collect(with_timestamp.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                        );

                        let transitions = self.one_per_frame[0]
                            .forward_log()
                            .map(|ok| ok.map(|value| collect(value.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                        );

                        let transitions = self.one_per_frame[1]
                            .forward_log()
                            .map(|ok| ok.map(|value| collect(value.value)));
                        assert_eq!(
                            transitions, expected_transitions,
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                        );
                    }
                    Err(OutOfLog) => {
                        assert!(
                            self.with_timestamp[0].forward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.with_timestamp[0],
                            self.with_timestamp[0]
                        );
                        assert!(
                            self.with_timestamp[1].forward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.with_timestamp[1],
                            self.with_timestamp[1]
                        );
                        assert!(
                            self.one_per_frame[0].forward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.one_per_frame[0],
                            self.one_per_frame[0]
                        );
                        assert!(
                            self.one_per_frame[1].forward_log().is_err(),
                            "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                            self.meta,
                            previous.one_per_frame[1],
                            self.one_per_frame[1]
                        );
                    }
                }
            }
        }

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
