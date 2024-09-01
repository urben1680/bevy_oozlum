use std::{
    collections::{TryReserveError, VecDeque},
    fmt::Debug,
};

use crate::meta::RevMeta;

use super::{
    AmountErr, DataEntry, LogIter, LogMut, OutOfLog, Packed, ValueLog, WithAmount, WithTimestamp,
};

#[derive(Debug, Clone)]
pub struct ValuesLog<T, U: Copy = (), Amount = usize>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
{
    amounts: ValueLog<WithAmount<U, Amount>>,
    values: VecDeque<T>,
    index: usize,
}

impl<T, U: Copy + Default, Amount> Default for ValuesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
{
    fn default() -> Self {
        Self::new_empty(U::default())
    }
}

impl<T, U: Copy, Amount> ValuesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
{
    pub fn new(
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<impl LogIter<'static, T>, U, Amount>> {
        let values = VecDeque::from_iter(iter.into_iter());
        let amount = match values.len().try_into() {
            Ok(amount) => Packed(amount),
            Err(err) => {
                return Err(AmountErr {
                    data: values.into_iter(),
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
    ) -> Result<Self, AmountErr<impl LogIter<'static, T>, U, Amount>> {
        let mut values = VecDeque::with_capacity(values_capacity);
        values.extend(iter.into_iter());
        let amount = match values.len().try_into() {
            Ok(amount) => Packed(amount),
            Err(err) => {
                return Err(AmountErr {
                    data: values.into_iter(),
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
    pub fn get(&self) -> (impl LogIter<&T>, U) {
        let entry = self.amounts.get();
        let to = self.index + entry.amount();
        let values = self.values.range(self.index..to);
        (values, entry.entry)
    }
    pub fn unlogged_get_mut(&mut self) -> (impl LogIter<&mut T>, U) {
        // todo: U is #[repr(packed)] so no mutable reference can be returned.
        // return a wrapper instead that contains a copied U and make it apply it's value to the entry on Drop.
        let with_amount = self.amounts.get();
        let to = self.index + with_amount.amount();
        let values = self.values.range_mut(self.index..to);
        (values, with_amount.entry)
    }
    pub fn past_end(&self) -> Option<(impl LogIter<&T>, U)> {
        let with_amount = self.amounts.past_end()?;
        let to = with_amount.amount();
        let values = self.values.range(..to);
        Some((values, with_amount.entry))
    }
    pub fn pop_past(&mut self) -> Option<DataEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<impl LogIter<T>, U, Amount>> {
        self.values.truncate(self.index);
        let entry = c(LogMut(&mut self.values)).into();
        let new_amount = self.values.len() - self.index;
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.values.len();
                self.amounts.push_present(WithAmount {
                    entry,
                    amount: Packed(amount),
                });
                Ok(())
            }
            Err(err) => Err(AmountErr {
                entry,
                data: self.values.drain(self.index..),
                err,
            }),
        }
    }
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<U>) {
        (
            self.values.drain(self.index..),
            self.amounts
                .drain_future()
                .map(|with_amount| with_amount.entry),
        )
    }
    pub fn clear(
        &mut self,
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<(), AmountErr<impl LogIter<'static, T>, U, Amount>> {
        let mut values = VecDeque::from_iter(iter.into_iter());
        let amount = match values.len().try_into() {
            Ok(amount) => Packed(amount),
            Err(err) => {
                return Err(AmountErr {
                    data: values.into_iter(),
                    entry,
                    err,
                })
            }
        };
        self.values.clear();
        self.values.append(&mut values);
        self.amounts.clear(WithAmount { entry, amount });
        self.index = 0;
        Ok(())
    }
    pub fn clear_empty(&mut self, entry: U) {
        self.values.clear();
        self.amounts.clear(WithAmount::zero(entry));
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        self.amounts.backward_log()?;
        let with_amount = self.amounts.get();
        self.index -= with_amount.amount();
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        self.amounts.forward_log()?;
        let with_amount = self.amounts.get();
        self.index += with_amount.amount();
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
            .map(WithAmount::amount)
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

impl<T, U: Copy, Amount> ValuesLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
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
            .map(WithAmount::amount)
            .sum();
        self.index -= amount;
        self.values.drain(..amount)
    }
}
