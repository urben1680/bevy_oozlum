use std::{
    collections::{TryReserveError, VecDeque},
    convert::Infallible,
    fmt::Debug,
};

use bevy::reflect::Reflect;

use super::{
    impl_with_amount, into_ok, AmountErr, BorrowTimestamp, EntryAmount, LogIter, LogMut, NotUSize,
    OutOfLog, RareStateLog, ValueEntry, WithAmount,
};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareStatesLog<T, U = (), const AMOUNT_BYTES: usize = 0>
where
    Self: WithAmount,
{
    amounts: RareStateLog<EntryAmount<U, <Self as WithAmount>::Amount>>,
    states: VecDeque<T>,
    index: usize,
}

impl_with_amount!(RareStatesLog);

impl<T, U: Default, const AMOUNT_BYTES: usize> Default for RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount,
{
    fn default() -> Self {
        Self::new_empty(U::default())
    }
}

impl<T, U, const AMOUNT_BYTES: usize> RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount,
{
    pub fn new_empty(entry: U) -> Self {
        Self {
            amounts: RareStateLog::new(EntryAmount::zero::<Self>(entry)),
            states: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities_empty(entry: U, states_capacity: usize, log_capacity: usize) -> Self {
        Self {
            amounts: RareStateLog::with_capacity(EntryAmount::zero::<Self>(entry), log_capacity),
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
        let entry_amount = self.amounts.get();
        let from = self.index - entry_amount.amount::<Self>();
        let states = self.states.range(from..self.index);
        (states, &entry_amount.entry)
    }
    pub fn unlogged_get_mut(&mut self) -> (impl LogIter<&mut T>, &mut U) {
        let entry_amount = self.amounts.unlogged_get_mut();
        let from = self.index - entry_amount.amount::<Self>();
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
        let amount = self.amounts.get().amount;
        let amount = <Self as WithAmount>::amount_to_usize(amount);
        self.states.drain(..self.index);
        self.states.truncate(amount);
        self.index = 0;
    }
    pub fn clear_empty(&mut self, entry: U) {
        self.states.clear();
        self.amounts.clear_with(EntryAmount::zero::<Self>(entry));
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        let amount = self.amounts.get().amount::<Self>();
        self.amounts.backward_log()?;
        self.index -= amount;
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        self.amounts.forward_log()?;
        self.index += self.amounts.get().amount::<Self>();
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

impl<T, U, const AMOUNT_BYTES: usize> RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Err = Infallible>,
{
    pub fn new(iter: impl IntoIterator<Item = T>, entry: U) -> Self {
        let states = VecDeque::from_iter(iter.into_iter());
        let pushed_amount = states.len();
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        Self {
            amounts: RareStateLog::new(EntryAmount { entry, amount }),
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
        states.extend(iter.into_iter());
        let pushed_amount = states.len();
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        Self {
            amounts: RareStateLog::with_capacity(EntryAmount { entry, amount }, log_capacity),
            states,
            index: 0,
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) -> Option<U> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        if pushed_amount == 0 {
            self.amounts.push_present(None);
            return Some(entry);
        }
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        self.index = self.states.len();
        self.amounts
            .push_present(Some(EntryAmount { entry, amount }));
        None
    }
    pub fn clear_with(&mut self, iter: impl IntoIterator<Item = T>, entry: U) {
        let mut states = VecDeque::from_iter(iter.into_iter());
        let pushed_amount = states.len();
        let amount = into_ok(<Self as WithAmount>::usize_to_amount(pushed_amount));
        self.states.clear();
        self.states.append(&mut states);
        self.amounts.clear_with(EntryAmount { entry, amount });
        self.index = 0;
    }
}

impl<T, U, const AMOUNT_BYTES: usize> RareStatesLog<T, U, AMOUNT_BYTES>
where
    Self: WithAmount<Amount: NotUSize>,
{
    pub fn try_new(
        iter: impl IntoIterator<Item = T>,
        entry: U,
    ) -> Result<Self, AmountErr<VecDeque<T>, U>> {
        let states = VecDeque::from_iter(iter.into_iter());
        let pushed_amount = states.len();
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => Ok(Self {
                amounts: RareStateLog::new(EntryAmount { entry, amount }),
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
        states.extend(iter.into_iter());
        let pushed_amount = states.len();
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => Ok(Self {
                amounts: RareStateLog::with_capacity(EntryAmount { entry, amount }, log_capacity),
                states,
                index: 0,
            }),
            Err(_) => Err(AmountErr::new::<Self>(states, entry, pushed_amount)),
        }
    }
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<Option<U>, AmountErr<impl LogIter<T>, U>> {
        self.states.truncate(self.index);
        let entry = c(LogMut(&mut self.states)).into();
        let pushed_amount = self.states.len() - self.index;
        if pushed_amount == 0 {
            self.amounts.push_present(None);
            return Ok(Some(entry));
        }
        match <Self as WithAmount>::usize_to_amount(pushed_amount) {
            Ok(amount) => {
                self.index = self.states.len();
                self.amounts
                    .push_present(Some(EntryAmount { entry, amount }));
                Ok(None)
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
        let mut states = VecDeque::from_iter(iter.into_iter());
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

impl<T, B: BorrowTimestamp, const AMOUNT_BYTES: usize> RareStatesLog<T, B, AMOUNT_BYTES>
where
    Self: WithAmount,
{
    pub fn pop_past_by_timestamp(
        &mut self,
        log_start: usize,
    ) -> Option<ValueEntry<impl LogIter<T>, B>> {
        self.amounts
            .pop_past_by_timestamp(log_start)
            .map(|entry_amount| self.drain_past_by_amount(entry_amount))
    }
    pub fn drain_past_by_timestamp(&mut self, log_start: usize) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_timestamp(log_start)
            .map(|entry_amount| entry_amount.amount::<Self>())
            .sum();
        self.index -= amount;
        self.states.drain(..amount)
    }
    pub fn reduce_timestamps(&mut self, by: usize) -> impl LogIter<T> {
        let amount = self
            .amounts
            .reduce_timestamps(by)
            .map(|entry_amount| entry_amount.amount::<Self>())
            .sum::<usize>();
        self.index -= amount;
        self.states.drain(..amount)
    }
}
