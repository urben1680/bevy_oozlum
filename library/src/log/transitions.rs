use std::{
    collections::{TryReserveError, VecDeque},
    convert::Infallible,
    fmt::Debug,
};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

use crate::meta::RevMeta;

use super::{
    doc_with_amount, impl_with_amount, AmountErr, EntryAmount, LogIter, LogMut, LoggedAt, NotUSize,
    OutOfLog, TransitionLog, ValueEntry, WithAmount,
};

#[doc = doc_with_amount!(struct)]
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionsLog<T, U = (), const AMOUNT_BYTES: usize = 0>
where
    Self: WithAmount<Entry = U>,
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

    use super::{EntryAmount, TransitionLog, TransitionsLog, WithAmount};

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for TransitionsLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmount<Entry = U>,
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
            TransitionLog::from_with_capacity(with_capacity.0)
                .map(|amounts| {
                    Self {
                        amounts,
                        transitions: with_capacity.1.0,
                        index: with_capacity.2,
                    }
                })
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity for TransitionsLog<T, U, AMOUNT_BYTES>
    where
        Self: WithAmount<Entry = U>,
    {
        type Se<'se> = (usize, usize) where T: 'se, U: 'se;
        type De = (usize, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (self.amounts.capacity(), self.transitions.capacity())
        }
        fn from_logless_with_capacity((log_capacity, transitions_capacity): Self::De) -> Result<Self, String> {
            Ok(Self::with_capacities(log_capacity, transitions_capacity))
        }
    }
}

impl_with_amount!(TransitionsLog);

impl<T, U, const AMOUNT_BYTES: usize> Default for TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Entry = U>,
{
    fn default() -> Self {
        Self::new()
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Entry = U>,
{
    pub const fn new() -> Self {
        Self {
            amounts: TransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(log_capacity: usize, transitions_capacity: usize) -> Self {
        Self {
            amounts: TransitionLog::with_capacity(log_capacity),
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.amounts.len()
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn log_capacity(&self) -> usize {
        self.amounts.capacity()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.amounts.is_empty()
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn log_reserve(&mut self, additional: usize) {
        self.amounts.reserve(additional)
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn log_reserve_exact(&mut self, additional: usize) {
        self.amounts.reserve_exact(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn log_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn log_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve_exact(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn log_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn log_shrink_to_fit(&mut self) {
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
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<U>) {
        (
            self.transitions.drain(self.index..),
            self.amounts
                .drain_future()
                .map(|entry_amount| entry_amount.entry),
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
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.index = self.transitions.len();
                self.amounts.push_present(EntryAmount { entry, amount });
                Ok(())
            }
            Err(error) => {
                let transitions = self.transitions.drain(self.index..);
                Err(AmountErr::new(
                    transitions,
                    entry,
                    pushed_amount,
                    error,
                ))
            }
        }
    }
}

#[doc = doc_with_amount!(impl where Infallible)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Entry = U, Err = Infallible>,
{
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        // rust analyzer does not like `let Ok(ok) = result;` here
        // https://github.com/rust-lang/rust-analyzer/issues/18334
        match self.fallible_push_present(c) {
            Ok(()) => (),
            Err(err) => match err._error {}
        }
    }
}

#[doc = doc_with_amount!(impl where NotUsize)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> TransitionsLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Entry = U, Amount: NotUSize>,
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
    Self: WithAmount<Entry = U>,
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

    use super::*;

    use crate::{log::LoggedAt, meta::RevMeta, RevFrame};

    /*
    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [TransitionsLog<usize, GetLoggedAt, 1>; 2],
        one_per_frame: [TransitionsLog<usize, (), 1>; 2],
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
            expected_len: usize,
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
            assert_eq!(
                self.with_timestamp[0].log_len(),
                expected_len,
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
            assert_eq!(
                self.with_timestamp[1].log_len(),
                expected_len,
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
                self.meta, previous.one_per_frame[0], self.one_per_frame[0]
            );
            assert_eq!(
                self.one_per_frame[0].log_len(),
                expected_len,
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
                self.meta, previous.one_per_frame[1], self.one_per_frame[1]
            );
            assert_eq!(
                self.one_per_frame[1].log_len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
        }
        fn backward_log<const N: usize>(
            &mut self,
            expected_transitions: Result<[usize; N], [OutOfLog; N]>,
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

                    let transitions = self.with_timestamp[0]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    let transitions = self.with_timestamp[1]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    let transitions = self.one_per_frame[0]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    let transitions = self.one_per_frame[1]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(_) => {
                    assert_eq!(
                        self.with_timestamp[0].backward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].backward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].backward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].backward_log().err(),
                        Some(OutOfLog),
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
            expected_transitions: Result<[usize; N], [OutOfLog; N]>,
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

                    let transitions = self.with_timestamp[0]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    let transitions = self.with_timestamp[1]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    let transitions = self.one_per_frame[0]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    let transitions = self.one_per_frame[1]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(_) => {
                    assert_eq!(
                        self.with_timestamp[0].forward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].forward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].forward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].forward_log().err(),
                        Some(OutOfLog),
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

        meta_and_logs.forward(Ok([1; 1]), 1);
        meta_and_logs.forward(Ok([2; 2]), 2);
        // pop_front called internally
        meta_and_logs.forward(Ok([3; 3]), 2);

        meta_and_logs.backward_log(Ok([3; 3]));
        meta_and_logs.backward_log(Ok([2; 2]));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err([OutOfLog]));

        meta_and_logs.forward_log(Ok([2; 2]));
        meta_and_logs.forward_log(Ok([3; 3]));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log(Err([OutOfLog]));

        meta_and_logs.backward_log(Ok([3; 3]));
        meta_and_logs.backward_log(Ok([2; 2]));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(Ok([4; 4]), 1);

        // amount of transitions is stored as u8, cannot store more than 255 transitions per push
        meta_and_logs.forward(Err([256; 256]), 1);
    }
    */

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<TransitionsLog<usize, RevFrame, 1>>();
    }
}
