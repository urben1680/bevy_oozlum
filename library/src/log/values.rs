use std::{
    collections::{TryReserveError, VecDeque},
    fmt::Debug,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{
    AmountErr, DataEntry, LogIter, LogMut, OutOfLog, PackedUSize, ValueLog, WithAmount,
    WithTimestamp,
};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ValuesLog<T, U = (), Amount = PackedUSize>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    amounts: ValueLog<WithAmount<U, Amount>>,
    values: VecDeque<T>,
    index: usize,
}

impl<T, U: Default, Amount> Default for ValuesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Default + Copy,
{
    fn default() -> Self {
        Self::new_empty(U::default())
    }
}

impl<T, U, Amount> ValuesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    pub fn new(
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<VecDeque<T>, U, Amount>> {
        let values = VecDeque::from_iter(iter.into_iter());
        let amount = match values.len().try_into() {
            Ok(amount) => amount,
            Err(err) => {
                return Err(AmountErr {
                    data: values,
                    entry,
                    err,
                })
            }
        };
        Ok(Self {
            amounts: ValueLog::new(WithAmount { entry, amount }),
            values,
            index: 0,
        })
    }
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: ValueLog::new(WithAmount::zero(entry)),
            values: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(
        iter: impl IntoIterator<Item = T>,
        entry: U,
        values_capacity: usize,
        log_capacity: usize,
    ) -> Result<Self, AmountErr<VecDeque<T>, U, Amount>> {
        let mut values = VecDeque::with_capacity(values_capacity);
        values.extend(iter.into_iter());
        let amount = match values.len().try_into() {
            Ok(amount) => amount,
            Err(err) => {
                return Err(AmountErr {
                    data: values,
                    entry,
                    err,
                })
            }
        };
        Ok(Self {
            amounts: ValueLog::with_capacity(WithAmount { entry, amount }, log_capacity),
            values,
            index: 0,
        })
    }
    pub fn with_capacities_empty(entry: U, values_capacity: usize, log_capacity: usize) -> Self {
        Self {
            amounts: ValueLog::with_capacity(WithAmount::zero(entry), log_capacity),
            values: VecDeque::with_capacity(values_capacity),
            index: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.amounts.len()
    }
    pub fn values_len(&self) -> usize {
        self.values.len()
    }
    pub fn log_capacity(&self) -> usize {
        self.amounts.capacity()
    }
    pub fn values_capacity(&self) -> usize {
        self.values.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.amounts.is_empty()
    }
    pub fn values_is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn log_reserve(&mut self, additional: usize) {
        self.amounts.reserve(additional)
    }
    pub fn values_reserve(&mut self, additional: usize) {
        self.values.reserve(additional)
    }
    pub fn log_reserve_exact(&mut self, additional: usize) {
        self.amounts.reserve_exact(additional)
    }
    pub fn values_reserve_exact(&mut self, additional: usize) {
        self.values.reserve_exact(additional)
    }
    pub fn log_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve(additional)
    }
    pub fn values_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve(additional)
    }
    pub fn log_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve_exact(additional)
    }
    pub fn values_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve_exact(additional)
    }
    pub fn log_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.shrink_to(min_capacity)
    }
    pub fn values_shrink_to(&mut self, min_capacity: usize) {
        self.values.shrink_to(min_capacity)
    }
    pub fn log_shrink_to_fit(&mut self) {
        self.amounts.shrink_to_fit()
    }
    pub fn values_shrink_to_fit(&mut self) {
        self.values.shrink_to_fit()
    }
    pub fn get(&self) -> (impl LogIter<&T>, &U) {
        let with_amount = self.amounts.get();
        let from = self.index - with_amount.amount();
        let values = self.values.range(from..self.index);
        (values, &with_amount.entry)
    }
    pub fn unlogged_get_mut(&mut self) -> (impl LogIter<&mut T>, &mut U) {
        let with_amount = self.amounts.unlogged_get_mut();
        let from = self.index - with_amount.amount();
        let values = self.values.range_mut(from..self.index);
        (values, &mut with_amount.entry)
    }
    pub fn past_end(&self) -> Option<(impl LogIter<&T>, &U)> {
        let with_amount = self.amounts.past_end()?;
        let to = with_amount.amount();
        let values = self.values.range(..to);
        Some((values, &with_amount.entry))
    }
    pub fn pop_past(&mut self) -> Option<DataEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<impl LogIter<T>, U, Amount>> {
        self.values.truncate(self.index);
        let entry = c(LogMut(&mut self.values)).into();
        let new_amount = self.values.len() - self.index;
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.values.len();
                self.amounts.push_present(WithAmount { entry, amount });
                Ok(())
            }
            Err(err) => Err(AmountErr {
                entry,
                data: self.values.drain(self.index..),
                err,
            }),
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        use std::any::type_name;
        self.try_push_present(c).unwrap_or_else(|err| {
            panic!("Tried to push {} values into {} which does not fit into {}. If the pushed amount is uncertain, use `try_push_present` or a larger `Amount` type.",
            err.data.len(), type_name::<Self>(), type_name::<Amount>()
            )
        })
    }
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<U>) {
        (
            self.values.drain(self.index..),
            self.amounts
                .drain_future()
                .map(|with_amount| with_amount.entry),
        )
    }
    pub fn clear(&mut self) {
        self.amounts.clear();
        let amount = self.amounts.get().amount;
        self.values.drain(..self.index);
        self.values.truncate(amount.into());
        self.index = 0;
    }
    pub fn try_clear_with(
        &mut self,
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), AmountErr<VecDeque<T>, U, Amount>> {
        let mut values = VecDeque::from_iter(iter.into_iter());
        let amount = match values.len().try_into() {
            Ok(amount) => amount,
            Err(err) => {
                return Err(AmountErr {
                    data: values,
                    entry,
                    err,
                })
            }
        };
        self.values.clear();
        self.values.append(&mut values);
        self.amounts.clear_with(WithAmount { entry, amount });
        self.index = 0;
        Ok(())
    }
    pub fn clear_empty(&mut self, entry: U) {
        self.values.clear();
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
        meta: &RevMeta,
        pushes_per_frame: usize,
    ) -> Option<DataEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past_by_len(meta, pushes_per_frame)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_len(
        &mut self,
        meta: &RevMeta,
        pushes_per_frame: usize,
    ) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_len(meta, pushes_per_frame)
            .map(|with_amount| with_amount.amount())
            .sum();
        self.index -= amount;
        self.values.drain(..amount)
    }
    fn drain_past_by_amount(
        &mut self,
        with_amount: WithAmount<U, Amount>,
    ) -> DataEntry<impl LogIter<T>, U> {
        let amount = with_amount.amount();
        self.index -= amount;
        DataEntry {
            data: self.values.drain(..amount),
            entry: with_amount.entry,
        }
    }
}

impl<T, U, Amount> ValuesLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    pub fn pop_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<DataEntry<impl LogIter<T>, WithTimestamp<U>>> {
        self.amounts
            .pop_past_by_timestamp(meta)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_timestamp(&mut self, meta: &RevMeta) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_timestamp(meta)
            .map(|with_amount| with_amount.amount())
            .sum();
        self.index -= amount;
        self.values.drain(..amount)
    }
}

#[cfg(test)]
mod test {
    use std::{borrow::Borrow, num::NonZeroUsize};

    use super::*;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [ValuesLog<usize, WithTimestamp, u8>; 2],
        one_per_frame: [ValuesLog<usize, (), u8>; 2],
    }

    fn collect(iter: impl IntoIterator<Item: Borrow<usize>>) -> Vec<usize> {
        iter.into_iter().map(|val| val.borrow().clone()).collect()
    }

    impl MetaAndLogs {
        fn new<const N: usize>(present: [usize; N], max_len: Option<NonZeroUsize>) -> Self {
            let meta = RevMeta::new(max_len, 0, false);
            let with_timestamp =
                ValuesLog::<usize, WithTimestamp, u8>::new(present, meta.with_timestamp(()))
                    .unwrap();
            let one_per_frame = ValuesLog::new(present, ()).unwrap();
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: [with_timestamp.clone(), with_timestamp],
                one_per_frame: [one_per_frame.clone(), one_per_frame],
            }
        }
        fn forward<const N: usize>(
            &mut self,
            values: Result<[usize; N], [usize; N]>,
            expected_log_len: usize,
        ) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update();

            let (values, expected_ok) = match values {
                Ok(values) => (values, true),
                Err(values) => (values, false),
            };

            let is_ok = self.with_timestamp[0]
                .try_push_present(|mut log| {
                    log.extend(values);
                    self.meta.with_timestamp(())
                })
                .is_ok();
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].pop_past_by_timestamp(&self.meta);
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
                    collect(values),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[0],
                    self.with_timestamp[0]
                );
            }

            let is_ok = self.with_timestamp[1]
                .try_push_present(|mut log| {
                    log.extend(values);
                    self.meta.with_timestamp(())
                })
                .is_ok();
            let middle = self.with_timestamp[1].clone();
            let _ = self.with_timestamp[1].drain_past_by_timestamp(&self.meta);
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
                    collect(values),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.with_timestamp[1],
                    self.with_timestamp[1]
                );
            }

            let is_ok = self.one_per_frame[0]
                .try_push_present(|mut log| log.extend(values))
                .is_ok();
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(&self.meta, 1);
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
                    collect(values),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[0],
                    self.one_per_frame[0]
                );
            }

            let is_ok = self.one_per_frame[1]
                .try_push_present(|mut log| log.extend(values))
                .is_ok();
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(&self.meta, 1);
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
                    collect(values),
                    "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                    self.meta,
                    previous.one_per_frame[1],
                    self.one_per_frame[1]
                );
            }
        }
        fn backward_log<const N: usize>(
            &mut self,
            expected_values: Result<[usize; N], [OutOfLog; N]>,
        ) {
            let previous = self.clone();

            match expected_values {
                Ok(expected_values) => {
                    let expected_values = collect(expected_values);
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    let is_ok = self.with_timestamp[0].backward_log().is_ok();
                    let values = collect(self.with_timestamp[0].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let is_ok = self.with_timestamp[1].backward_log().is_ok();
                    let values = collect(self.with_timestamp[1].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let is_ok = self.one_per_frame[0].backward_log().is_ok();
                    let values = collect(self.one_per_frame[0].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let is_ok = self.one_per_frame[1].backward_log().is_ok();
                    let values = collect(self.one_per_frame[1].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                }
                Err(_) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_err(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
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
            expected_values: Result<[usize; N], [OutOfLog; N]>,
        ) {
            let previous = self.clone();
            match expected_values {
                Ok(expected_values) => {
                    let expected_values = collect(expected_values);
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    let is_ok = self.with_timestamp[0].forward_log().is_ok();
                    let values = collect(self.with_timestamp[0].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let is_ok = self.with_timestamp[1].forward_log().is_ok();
                    let values = collect(self.with_timestamp[1].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let is_ok = self.one_per_frame[0].forward_log().is_ok();
                    let values = collect(self.one_per_frame[0].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let is_ok = self.one_per_frame[1].forward_log().is_ok();
                    let values = collect(self.one_per_frame[1].get().0);
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                    assert_eq!(
                        values, expected_values,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                }
                Err(_) => {
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

        // amount of values is stored as u8, cannot store more than 255 values per push
        meta_and_logs.forward(Err([256; 256]), 1);
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<ValuesLog<usize, WithTimestamp<u8>, u8>>();
    }
}
