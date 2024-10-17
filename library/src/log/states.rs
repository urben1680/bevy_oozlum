use std::{
    collections::{TryReserveError, VecDeque},
    convert::Infallible,
    fmt::Debug,
    ops::Range,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{
    doc_with_amount, impl_with_amount, into_ok, AmountErr, EntryAmount, LogIter, LogMut, LoggedAt,
    NotUSize, OutOfLog, StateLog, ValueEntry, WithAmount,
};

#[doc = doc_with_amount!(struct)]
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatesLog<T, U = (), const AMOUNT_BYTES: usize = 0>
where
    Self: WithAmount,
{
    amounts: StateLog<EntryAmount<U, <Self as WithAmount>::Amount>>,
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

    use super::{EntryAmount, StateLog, StatesLog, WithAmount};

    impl<T, U, const AMOUNT_BYTES: usize> LoglessState for StatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmount,
    {
        type Se<'se> = (
            &'se EntryAmount<U, <Self as WithAmount>::Amount>,
            WithRange<'se, T>,
        );
        type De = (EntryAmount<U, <Self as WithAmount>::Amount>, VecDeque<T>);
        fn get_logless_state(&self) -> Self::Se<'_> {
            let amounts_se = self.amounts.get_logless_state();
            let range = self.get_range_entry().0;
            (
                amounts_se,
                WithRange {
                    deque: &self.states,
                    range,
                },
            )
        }
        fn from_logless_state(logless_state: Self::De) -> Self {
            let amounts = <
                StateLog<EntryAmount<U, <Self as WithAmount>::Amount>> as LoglessState
            >::from_logless_state(logless_state.0);
            Self {
                states: logless_state.1,
                amounts,
                index: 0,
            }
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> WithCapacity for StatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmount,
    {
        type Se<'se> = (
            (
                WithCapacityWrapper<&'se VecDeque<EntryAmount<U, <Self as WithAmount>::Amount>>>,
                &'se EntryAmount<U, <Self as WithAmount>::Amount>,
                usize,
            ),
            WithCapacityWrapper<&'se VecDeque<T>>,
            usize,
        );
        type De = (
            (
                WithCapacityWrapper<VecDeque<EntryAmount<U, <Self as WithAmount>::Amount>>>,
                EntryAmount<U, <Self as WithAmount>::Amount>,
                usize,
            ),
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
        fn from_with_capacity(with_capacity: Self::De) -> Self {
            let amounts = WithCapacity::from_with_capacity(with_capacity.0);
            let states = with_capacity.1 .0;
            let index = with_capacity.2;
            Self {
                amounts,
                states,
                index,
            }
        }
    }

    impl<T, U, const AMOUNT_BYTES: usize> LoglessWithCapacity for StatesLog<T, U, AMOUNT_BYTES>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
        U: Serialize + for<'de> Deserialize<'de> + 'static,
        Self: WithAmount,
    {
        type Se<'se> = (
            (&'se EntryAmount<U, <Self as WithAmount>::Amount>, usize),
            WithCapacityWrapper<WithRange<'se, T>>,
        );
        type De = (
            (EntryAmount<U, <Self as WithAmount>::Amount>, usize),
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
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            let amounts = StateLog::from_logless_with_capacity(logless_with_capacity.0);
            Self {
                amounts,
                states: logless_with_capacity.1 .0,
                index: 0,
            }
        }
    }
}

impl_with_amount!(StatesLog);

impl<T, U: Default, const AMOUNT_BYTES: usize> Default for StatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount,
{
    fn default() -> Self {
        Self::new_empty(U::default())
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> StatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount,
{
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: StateLog::new(EntryAmount::zero::<Self>(entry)),
            states: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities_empty(entry: U, states_capacity: usize, log_capacity: usize) -> Self {
        Self {
            amounts: StateLog::with_capacity(EntryAmount::zero::<Self>(entry), log_capacity),
            states: VecDeque::with_capacity(states_capacity),
            index: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.amounts.len()
    }
    pub fn states_len(&self) -> usize {
        self.states.len()
    }
    pub fn log_capacity(&self) -> usize {
        self.amounts.capacity()
    }
    pub fn states_capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.amounts.is_empty()
    }
    pub fn states_is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn log_reserve(&mut self, additional: usize) {
        self.amounts.reserve(additional)
    }
    pub fn states_reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn log_reserve_exact(&mut self, additional: usize) {
        self.amounts.reserve_exact(additional)
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn log_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve(additional)
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn log_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve_exact(additional)
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn log_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.shrink_to(min_capacity)
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn log_shrink_to_fit(&mut self) {
        self.amounts.shrink_to_fit()
    }
    pub fn states_shrink_to_fit(&mut self) {
        self.states.shrink_to_fit()
    }
    fn get_range_entry(&self) -> (Range<usize>, &U) {
        let entry_amount = &self.amounts;
        let amount = entry_amount.amount::<Self>();
        let from = self.index - amount;
        (from..self.index, &entry_amount.entry)
    }
    pub fn get(&self) -> (impl LogIter<&T>, &U) {
        let (range, entry) = self.get_range_entry();
        let states = self.states.range(range);
        (states, entry)
    }
    pub fn unlogged_get_mut(&mut self) -> (impl LogIter<&mut T>, &mut U) {
        let entry_amount = self.amounts.unlogged_get_mut();
        let amount = entry_amount.amount::<Self>();
        let from = self.index - amount;
        let states = self.states.range_mut(from..self.index);
        (states, &mut entry_amount.entry)
    }
    pub fn past_end(&self) -> Option<(impl LogIter<&T>, &U)> {
        let entry_amount = self.amounts.past_end()?;
        let to = entry_amount.amount::<Self>();
        let states = self.states.range(..to);
        Some((states, &entry_amount.entry))
    }
    pub fn pop_past(&mut self) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<U>) {
        (
            self.states.drain(self.index..),
            self.amounts
                .drain_future()
                .map(|entry_amount| entry_amount.entry),
        )
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        let amount = self.amounts.amount::<Self>();
        self.states.drain(..self.index);
        self.states.truncate(amount);
        self.index = 0;
    }
    pub fn clear_empty(&mut self, entry: U) {
        self.states.clear();
        let entry_amount = EntryAmount {
            entry,
            amount: <Self as WithAmount>::MIN,
        };
        self.amounts.clear_with(entry_amount);
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        let amount = self.amounts.amount::<Self>();
        self.amounts.backward_log()?;
        self.index -= amount;
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        self.amounts.forward_log()?;
        let amount = self.amounts.amount::<Self>();
        self.index += amount;
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
            .map(|entry_amount| entry_amount.amount::<Self>())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
    }
    fn drain_past_by_amount(
        &mut self,
        entry_amount: EntryAmount<U, <Self as WithAmount>::Amount>,
    ) -> ValueEntry<impl LogIter<T>, U> {
        let amount = entry_amount.amount::<Self>();
        self.index -= amount;
        ValueEntry {
            value: self.states.drain(..amount),
            entry: entry_amount.entry,
        }
    }
}

#[doc = doc_with_amount!(impl where Infallible)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> StatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Err = Infallible>,
{
    pub fn new(iter: impl IntoIterator<Item = T>, entry: U) -> Self {
        let states = VecDeque::from_iter(iter);
        let pushed_amount = states.len();
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        Self {
            amounts: StateLog::new(EntryAmount { entry, amount }),
            states,
            index: 0,
        }
    }
    pub fn with_capacities(
        iter: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        log_capacity: usize,
    ) -> Self {
        let mut states = VecDeque::with_capacity(states_capacity);
        states.extend(iter);
        let pushed_amount = states.len();
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        let entry_amount = EntryAmount { entry, amount };
        Self {
            amounts: StateLog::with_capacity(entry_amount, log_capacity),
            states,
            index: 0,
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        self.index = self.states.len();
        self.amounts.push_present(EntryAmount { entry, amount });
    }
    pub fn clear_with(&mut self, iter: impl IntoIterator<Item = T>, entry: U) {
        let mut states = VecDeque::from_iter(iter);
        let pushed_amount = states.len();
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        self.states.clear();
        self.states.append(&mut states);
        self.amounts.clear_with(EntryAmount { entry, amount });
        self.index = 0;
    }
}

#[doc = doc_with_amount!(impl where NotUsize)]
#[allow(private_bounds)]
impl<T, U, const AMOUNT_BYTES: usize> StatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Amount: NotUSize>,
{
    pub fn try_new(
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<VecDeque<T>, U>> {
        let states = VecDeque::from_iter(iter);
        let pushed_amount = states.len();
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => Ok(Self {
                amounts: StateLog::new(EntryAmount { entry, amount }),
                states,
                index: 0,
            }),
            Err(_) => Err(AmountErr::new::<Self>(states, entry, pushed_amount)),
        }
    }
    pub fn try_with_capacities(
        iter: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        log_capacity: usize,
    ) -> Result<Self, AmountErr<VecDeque<T>, U>> {
        let mut states = VecDeque::with_capacity(states_capacity);
        states.extend(iter);
        let pushed_amount = states.len();
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => Ok(Self {
                amounts: StateLog::with_capacity(EntryAmount { entry, amount }, log_capacity),
                states,
                index: 0,
            }),
            Err(_) => Err(AmountErr::new::<Self>(states, entry, pushed_amount)),
        }
    }
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<impl LogIter<T>, U>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.index = self.states.len();
                self.amounts.push_present(EntryAmount { entry, amount });
                Ok(())
            }
            Err(_) => {
                let states = self.states.drain(self.index..);
                Err(AmountErr::new::<Self>(states, entry, pushed_amount))
            }
        }
    }
    pub fn try_clear_with(
        &mut self,
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), AmountErr<VecDeque<T>, U>> {
        let mut states = VecDeque::from_iter(iter);
        let pushed_amount = states.len();
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.states.clear();
                self.states.append(&mut states);
                self.amounts.clear_with(EntryAmount { entry, amount });
                self.index = 0;
                Ok(())
            }
            Err(_) => Err(AmountErr::new::<Self>(states, entry, pushed_amount)),
        }
    }
}

#[doc = doc_with_amount!(impl)]
#[allow(private_bounds)]
impl<T, U: LoggedAt, const AMOUNT_BYTES: usize> StatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount,
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
            .map(|entry_amount| entry_amount.amount::<Self>())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
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
        with_timestamp: [StatesLog<usize, LoggedAt, 1>; 2],
        one_per_frame: [StatesLog<usize, (), 1>; 2],
    }

    fn collect(iter: impl IntoIterator<Item: Borrow<usize>>) -> Vec<usize> {
        iter.into_iter().map(|val| val.borrow().clone()).collect()
    }

    impl MetaAndLogs {
        fn new<const N: usize>(present: [usize; N], max_len: Option<NonZeroUsize>) -> Self {
            let meta = RevMeta::new(max_len, 0, false);
            let with_timestamp = StatesLog::try_new(present, meta.with_logged_at(())).unwrap();
            let one_per_frame = StatesLog::try_new(present, ()).unwrap();
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: [with_timestamp.clone(), with_timestamp],
                one_per_frame: [one_per_frame.clone(), one_per_frame],
            }
        }
        fn forward<const N: usize>(
            &mut self,
            states: Result<[usize; N], [usize; N]>,
            expected_log_len: usize,
        ) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update();

            let (states, expected_ok) = match states {
                Ok(states) => (states, true),
                Err(states) => (states, false),
            };

            let is_ok = self.with_timestamp[0]
                .try_push_present(|mut log| {
                    log.extend(states);
                    self.meta.with_logged_at(())
                })
                .is_ok();
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].pop_past_by_timestamp(*self.meta.log_range().start());
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.with_timestamp[0], self.with_timestamp[0]
            );
            assert_eq!(
                self.with_timestamp[0].log_len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );
            if expected_ok {
                assert_eq!(
                    collect(self.with_timestamp[0].get().0),
                    collect(states),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[0],
                    self.with_timestamp[0]
                );
            }

            let is_ok = self.with_timestamp[1]
                .try_push_present(|mut log| {
                    log.extend(states);
                    self.meta.with_logged_at(())
                })
                .is_ok();
            let middle = self.with_timestamp[1].clone();
            let _ = self.with_timestamp[1].drain_past_by_timestamp(*self.meta.log_range().start());
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.with_timestamp[1], self.with_timestamp[1]
            );
            assert_eq!(
                self.with_timestamp[1].log_len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );
            if expected_ok {
                assert_eq!(
                    collect(self.with_timestamp[1].get().0),
                    collect(states),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[1],
                    self.with_timestamp[1]
                );
            }

            let is_ok = self.one_per_frame[0]
                .try_push_present(|mut log| log.extend(states))
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
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );
            if expected_ok {
                assert_eq!(
                    collect(self.one_per_frame[0].get().0),
                    collect(states),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[0],
                    self.one_per_frame[0]
                );
            }

            let is_ok = self.one_per_frame[1]
                .try_push_present(|mut log| log.extend(states))
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
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
            if expected_ok {
                assert_eq!(
                    collect(self.one_per_frame[1].get().0),
                    collect(states),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[1],
                    self.one_per_frame[1]
                );
            }
        }
        fn backward_log<const N: usize>(
            &mut self,
            expected_states: Result<[usize; N], [OutOfLog; N]>,
        ) {
            let previous = self.clone();

            match expected_states {
                Ok(expected_states) => {
                    let expected_states = collect(expected_states);
                    assert!(
                        self.meta
                            .queue_log(self.meta.present_world_state() - 1)
                            .is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    assert_eq!(
                        self.with_timestamp[0].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        collect(self.with_timestamp[0].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    assert_eq!(
                        self.with_timestamp[1].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        collect(self.with_timestamp[1].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    assert_eq!(
                        self.one_per_frame[0].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        collect(self.one_per_frame[0].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    assert_eq!(
                        self.one_per_frame[1].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                    assert_eq!(
                        collect(self.one_per_frame[1].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(_) => {
                    assert_eq!(
                        self.with_timestamp[0].backward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].backward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].backward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].backward_log(),
                        Err(OutOfLog),
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
            expected_states: Result<[usize; N], [OutOfLog; N]>,
        ) {
            let previous = self.clone();
            match expected_states {
                Ok(expected_states) => {
                    let expected_states = collect(expected_states);
                    assert!(
                        self.meta
                            .queue_log(self.meta.present_world_state() + 1)
                            .is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    assert_eq!(
                        self.with_timestamp[0].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        collect(self.with_timestamp[0].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    assert_eq!(
                        self.with_timestamp[1].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        collect(self.with_timestamp[1].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    assert_eq!(
                        self.one_per_frame[0].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        collect(self.one_per_frame[0].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    assert_eq!(
                        self.one_per_frame[1].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                    assert_eq!(
                        collect(self.one_per_frame[1].get().0),
                        expected_states,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(_) => {
                    assert_eq!(
                        self.with_timestamp[0].forward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].forward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].forward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].forward_log(),
                        Err(OutOfLog),
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
        let mut meta_and_logs = MetaAndLogs::new([], NonZeroUsize::new(3));

        meta_and_logs.forward(Ok([1; 1]), 1);
        meta_and_logs.forward(Ok([2; 2]), 2);
        // pop_front called internally
        meta_and_logs.forward(Ok([3; 3]), 2);

        meta_and_logs.backward_log(Ok([2; 2]));
        meta_and_logs.backward_log(Ok([1; 1]));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err([OutOfLog]));

        meta_and_logs.forward_log(Ok([2; 2]));
        meta_and_logs.forward_log(Ok([3; 3]));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log(Err([OutOfLog]));

        meta_and_logs.backward_log(Ok([2; 2]));
        meta_and_logs.backward_log(Ok([1; 1]));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(Ok([4; 4]), 1);

        // amount of states is stored as u8, cannot store more than 255 states per push
        meta_and_logs.forward(Err([256; 256]), 1);
    }
    */

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<StatesLog<usize, RevFrame, 1>>();
    }
}
