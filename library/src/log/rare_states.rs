use std::{
    collections::{TryReserveError, VecDeque},
    fmt::Debug,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{
    AmountErr, ValueEntry, LogIter, LogMut, OutOfLog, PackedUSize, RareStateLog, WithAmount,
    WithTimestamp,
};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareStatesLog<T, U = (), Amount = PackedUSize>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    amounts: RareStateLog<WithAmount<U, Amount>>,
    states: VecDeque<T>,
    index: usize,
}

impl<T, U: Default, Amount> Default for RareStatesLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    fn default() -> Self {
        Self::new_empty(U::default())
    }
}

impl<T, U, Amount> RareStatesLog<T, U, Amount>
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
            amounts: RareStateLog::new(WithAmount { entry, amount }),
            states,
            index: 0,
        })
    }
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: RareStateLog::new(WithAmount::zero(entry)),
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
            amounts: RareStateLog::with_capacity(WithAmount { entry, amount }, log_capacity),
            states,
            index: 0,
        })
    }
    pub fn with_capacities_empty(entry: U, states_capacity: usize, log_capacity: usize) -> Self {
        Self {
            amounts: RareStateLog::with_capacity(WithAmount::zero(entry), log_capacity),
            states: VecDeque::with_capacity(states_capacity),
            index: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.amounts.log_len()
    }
    pub fn states_len(&self) -> usize {
        self.states.len()
    }
    pub fn log_capacity(&self) -> usize {
        self.amounts.states_capacity()
    }
    pub fn states_capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.amounts.states_is_empty()
    }
    pub fn states_is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn log_reserve(&mut self, additional: usize) {
        self.amounts.states_reserve(additional)
    }
    pub fn states_reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn log_reserve_exact(&mut self, additional: usize) {
        self.amounts.states_reserve_exact(additional)
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn log_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.states_try_reserve(additional)
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn log_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.states_try_reserve_exact(additional)
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn log_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.states_shrink_to(min_capacity)
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn log_shrink_to_fit(&mut self) {
        self.amounts.states_shrink_to_fit()
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
    ) -> Result<Option<U>, AmountErr<impl LogIter<T>, U, Amount>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let new_amount = self.states.len() - self.index;
        if new_amount == 0 {
            self.amounts.push_present(None);
            return Ok(Some(entry));
        }
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.states.len();
                self.amounts
                    .push_present(Some(WithAmount { entry, amount }));
                Ok(None)
            }
            Err(err) => Err(AmountErr {
                entry,
                values: self.states.drain(self.index..),
                err,
            }),
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) -> Option<U> {
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
    pub fn clear_with(
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

impl<T, U, Amount> RareStatesLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    pub fn pop_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<ValueEntry<impl LogIter<T>, WithTimestamp<U>>> {
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
        self.states.drain(..amount)
    }
}
