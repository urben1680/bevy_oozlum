use std::{
    collections::{TryReserveError, VecDeque},
    fmt::Debug,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{
    AmountErr, DataEntry, LogIter, LogMut, OutOfLog, PackedUSize, RareValueLog, WithAmount,
    WithTimestamp,
};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareValuesLog<T, U = (), Amount = PackedUSize>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    amounts: RareValueLog<WithAmount<U, Amount>>,
    values: VecDeque<T>,
    index: usize,
}

impl<T, U: Default, Amount> Default for RareValuesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    fn default() -> Self {
        Self::new_empty(U::default())
    }
}

impl<T, U, Amount> RareValuesLog<T, U, Amount>
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
            amounts: RareValueLog::new(WithAmount { entry, amount }),
            values,
            index: 0,
        })
    }
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: RareValueLog::new(WithAmount::zero(entry)),
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
            amounts: RareValueLog::with_capacity(WithAmount { entry, amount }, log_capacity),
            values,
            index: 0,
        })
    }
    pub fn with_capacities_empty(entry: U, values_capacity: usize, log_capacity: usize) -> Self {
        Self {
            amounts: RareValueLog::with_capacity(WithAmount::zero(entry), log_capacity),
            values: VecDeque::with_capacity(values_capacity),
            index: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.amounts.log_len()
    }
    pub fn values_len(&self) -> usize {
        self.values.len()
    }
    pub fn log_capacity(&self) -> usize {
        self.amounts.values_capacity()
    }
    pub fn values_capacity(&self) -> usize {
        self.values.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.amounts.values_is_empty()
    }
    pub fn values_is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn log_reserve(&mut self, additional: usize) {
        self.amounts.values_reserve(additional)
    }
    pub fn values_reserve(&mut self, additional: usize) {
        self.values.reserve(additional)
    }
    pub fn log_reserve_exact(&mut self, additional: usize) {
        self.amounts.values_reserve_exact(additional)
    }
    pub fn values_reserve_exact(&mut self, additional: usize) {
        self.values.reserve_exact(additional)
    }
    pub fn log_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.values_try_reserve(additional)
    }
    pub fn values_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve(additional)
    }
    pub fn log_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.values_try_reserve_exact(additional)
    }
    pub fn values_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve_exact(additional)
    }
    pub fn log_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.values_shrink_to(min_capacity)
    }
    pub fn values_shrink_to(&mut self, min_capacity: usize) {
        self.values.shrink_to(min_capacity)
    }
    pub fn log_shrink_to_fit(&mut self) {
        self.amounts.values_shrink_to_fit()
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
    ) -> Result<Option<U>, AmountErr<impl LogIter<T>, U, Amount>> {
        self.values.truncate(self.index);
        let entry = c(LogMut(&mut self.values)).into();
        let new_amount = self.values.len() - self.index;
        if new_amount == 0 {
            self.amounts.push_present(None);
            return Ok(Some(entry));
        }
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.values.len();
                self.amounts
                    .push_present(Some(WithAmount { entry, amount }));
                Ok(None)
            }
            Err(err) => Err(AmountErr {
                entry,
                data: self.values.drain(self.index..),
                err,
            }),
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) -> Option<U> {
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
    pub fn clear_with(
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

impl<T, U, Amount> RareValuesLog<T, WithTimestamp<U>, Amount>
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
