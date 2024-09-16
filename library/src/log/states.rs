use std::{
    collections::{TryReserveError, VecDeque},
    fmt::Debug,
};

use bevy::reflect::Reflect;

use super::{
    AmountErr, LogIter, LogMut, OutOfLog, PackedUSize, StateLog, ValueEntry, WithAmount,
    WithTimestamp,
};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StatesLog<T, U = (), Amount = PackedUSize>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    amounts: StateLog<WithAmount<U, Amount>>,
    states: VecDeque<T>,
    index: usize,
}

impl<T, U: Default, Amount> Default for StatesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Default + Copy,
{
    fn default() -> Self {
        Self::new_empty(U::default())
    }
}

impl<T, U, Amount> StatesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    pub fn new(
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<VecDeque<T>, U, Amount>> {
        let states = VecDeque::from_iter(iter.into_iter());
        let amount = match states.len().try_into() {
            Ok(amount) => amount,
            Err(err) => {
                return Err(AmountErr {
                    values: states,
                    entry,
                    err,
                })
            }
        };
        Ok(Self {
            amounts: StateLog::new(WithAmount { entry, amount }),
            states,
            index: 0,
        })
    }
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: StateLog::new(WithAmount::zero(entry)),
            states: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(
        iter: impl IntoIterator<Item = T>,
        entry: U,
        states_capacity: usize,
        log_capacity: usize,
    ) -> Result<Self, AmountErr<VecDeque<T>, U, Amount>> {
        let mut states = VecDeque::with_capacity(states_capacity);
        states.extend(iter.into_iter());
        let amount = match states.len().try_into() {
            Ok(amount) => amount,
            Err(err) => {
                return Err(AmountErr {
                    values: states,
                    entry,
                    err,
                })
            }
        };
        Ok(Self {
            amounts: StateLog::with_capacity(WithAmount { entry, amount }, log_capacity),
            states,
            index: 0,
        })
    }
    pub fn with_capacities_empty(entry: U, states_capacity: usize, log_capacity: usize) -> Self {
        Self {
            amounts: StateLog::with_capacity(WithAmount::zero(entry), log_capacity),
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
    pub fn get(&self) -> (impl LogIter<&T>, &U) {
        let with_amount = self.amounts.get();
        let from = self.index - with_amount.amount();
        let states = self.states.range(from..self.index);
        (states, &with_amount.entry)
    }
    pub fn unlogged_get_mut(&mut self) -> (impl LogIter<&mut T>, &mut U) {
        let with_amount = self.amounts.unlogged_get_mut();
        let from = self.index - with_amount.amount();
        let states = self.states.range_mut(from..self.index);
        (states, &mut with_amount.entry)
    }
    pub fn past_end(&self) -> Option<(impl LogIter<&T>, &U)> {
        let with_amount = self.amounts.past_end()?;
        let to = with_amount.amount();
        let states = self.states.range(..to);
        Some((states, &with_amount.entry))
    }
    pub fn pop_past(&mut self) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<impl LogIter<T>, U, Amount>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let new_amount = self.states.len() - self.index;
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.states.len();
                self.amounts.push_present(WithAmount { entry, amount });
                Ok(())
            }
            Err(err) => Err(AmountErr {
                entry,
                values: self.states.drain(self.index..),
                err,
            }),
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        use std::any::type_name;
        self.try_push_present(c).unwrap_or_else(|err| {
            panic!("Tried to push {} states into {} which does not fit into {}. If the pushed amount is uncertain, use `try_push_present` or a larger `Amount` type.",
            err.values.len(), type_name::<Self>(), type_name::<Amount>()
            )
        })
    }
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<U>) {
        (
            self.states.drain(self.index..),
            self.amounts
                .drain_future()
                .map(|with_amount| with_amount.entry),
        )
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        let amount = self.amounts.get().amount;
        self.states.drain(..self.index);
        self.states.truncate(amount.into());
        self.index = 0;
    }
    pub fn try_clear_with(
        &mut self,
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), AmountErr<VecDeque<T>, U, Amount>> {
        let mut states = VecDeque::from_iter(iter.into_iter());
        let amount = match states.len().try_into() {
            Ok(amount) => amount,
            Err(err) => {
                return Err(AmountErr {
                    values: states,
                    entry,
                    err,
                })
            }
        };
        self.states.clear();
        self.states.append(&mut states);
        self.amounts.clear_with(WithAmount { entry, amount });
        self.index = 0;
        Ok(())
    }
    pub fn clear_empty(&mut self, entry: U) {
        self.states.clear();
        self.amounts.clear_with(WithAmount::zero(entry));
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        let amount = self.amounts.get().amount();
        self.amounts.backward_log()?;
        self.index -= amount;
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        self.amounts.forward_log()?;
        self.index += self.amounts.get().amount();
        Ok(())
    }
    pub fn pop_past_by_len(
        &mut self,
        max_past_len: usize,
    ) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past_by_len(max_past_len)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_len(max_past_len)
            .map(|with_amount| with_amount.amount())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
    }
    fn drain_past_by_amount(
        &mut self,
        with_amount: WithAmount<U, Amount>,
    ) -> ValueEntry<impl LogIter<T>, U> {
        let amount = with_amount.amount();
        self.index -= amount;
        ValueEntry {
            value: self.states.drain(..amount),
            entry: with_amount.entry,
        }
    }
}

impl<T, U, Amount> StatesLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    pub fn pop_past_by_timestamp(
        &mut self,
        log_start: usize,
    ) -> Option<ValueEntry<impl LogIter<T>, WithTimestamp<U>>> {
        self.amounts
            .pop_past_by_timestamp(log_start)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_timestamp(&mut self, log_start: usize) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_timestamp(log_start)
            .map(|with_amount| with_amount.amount())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
    }
}

#[cfg(test)]
mod test {
    use std::{borrow::Borrow, num::NonZeroUsize};

    use super::*;

    use crate::meta::RevMeta;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [StatesLog<usize, WithTimestamp, u8>; 2],
        one_per_frame: [StatesLog<usize, (), u8>; 2],
    }

    fn collect(iter: impl IntoIterator<Item: Borrow<usize>>) -> Vec<usize> {
        iter.into_iter().map(|val| val.borrow().clone()).collect()
    }

    impl MetaAndLogs {
        fn new<const N: usize>(present: [usize; N], max_len: Option<NonZeroUsize>) -> Self {
            let meta = RevMeta::new(max_len, 0, false);
            let with_timestamp =
                StatesLog::<usize, WithTimestamp, u8>::new(present, meta.with_timestamp(()))
                    .unwrap();
            let one_per_frame = StatesLog::new(present, ()).unwrap();
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
                    self.meta.with_timestamp(())
                })
                .is_ok();
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].pop_past_by_timestamp(self.meta.log_range().start);
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
                    self.meta.with_timestamp(())
                })
                .is_ok();
            let middle = self.with_timestamp[1].clone();
            let _ = self.with_timestamp[1].drain_past_by_timestamp(self.meta.log_range().start);
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
            self.one_per_frame[0].pop_past_by_len(self.meta.past_len());
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
            let _ = self.one_per_frame[1].drain_past_by_len(self.meta.past_len());
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
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
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
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
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

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<StatesLog<usize, WithTimestamp<u8>, u8>>();
    }
}
