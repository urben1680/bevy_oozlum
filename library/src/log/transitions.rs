use std::{
    collections::{
        vec_deque::{Drain, IterMut},
        TryReserveError, VecDeque,
    },
    fmt::Debug,
    iter::Rev,
};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{
    amount_to_usize, should_pop_transition_at_push, AmountErr, DataEntry, LimitLen, LogMut,
    OutOfLog, TransitionLog, WithAmount, WithTimestamp,
};

#[derive(Debug, Component, Resource)]
pub struct TransitionsLog<T, U = (), Amount = usize>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    amounts: TransitionLog<WithAmount<U, Amount>>,
    transitions: VecDeque<T>,
    index: usize,
}

impl<T, U, Amount> Default for TransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U, Amount> TransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
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
    pub fn front(&self) -> Option<&WithAmount<U, Amount>> {
        self.amounts.front()
    }
    pub fn pop_front(&mut self) -> Option<DataEntry<Drain<T>, U>> {
        self.amounts.pop_front().map(|with_amount| {
            let amount = amount_to_usize(with_amount.amount);
            self.index -= amount;
            DataEntry {
                data: self.transitions.drain(..amount),
                entry: with_amount.entry,
            }
        })
    }
    // todo: extend methode die den letzten amount vergrößert?
    pub fn push_back(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> U,
    ) -> Result<(), AmountErr<T, U, Amount>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions));
        let new_amount = self.transitions.len() - self.index;
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.transitions.len();
                self.amounts.push_back(WithAmount { entry, amount });
                Ok(())
            }
            Err(err) => Err(AmountErr {
                entry,
                data: self.transitions.drain(self.index..),
                err,
            }),
        }
    }
    pub fn drain_future_transitions(&mut self) -> Rev<Drain<T>> {
        self.transitions.drain(self.index..).rev()
    }
    pub fn drain_future_entries(&mut self) -> Drain<WithAmount<U, Amount>> {
        self.amounts.drain_future()
    }
    pub fn backward_log(&mut self) -> Result<DataEntry<IterMut<T>, &U>, OutOfLog> {
        let old_index = self.index;
        let with_amount = self.amounts.backward_log()?;
        self.index -= amount_to_usize(with_amount.amount.clone());
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(DataEntry {
            data: iter,
            entry: &with_amount.entry,
        })
    }
    pub fn forward_log(&mut self) -> Result<DataEntry<IterMut<T>, &U>, OutOfLog> {
        let old_index = self.index;
        let with_amount = self.amounts.forward_log()?;
        self.index += amount_to_usize(with_amount.amount.clone());
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(DataEntry {
            data: iter,
            entry: &with_amount.entry,
        })
    }
}

impl<T, U, Amount> TransitionsLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    pub fn pop_front_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<DataEntry<Drain<T>, WithTimestamp<U>>> {
        if self.front().map_or(false, |with_amount| {
            // remove at range().start because this entry instructs how to transition from range().start to range().start - 1
            with_amount.entry.logged_at <= meta.range().start
        }) {
            self.pop_front()
        } else {
            None
        }
    }
}

impl<T, U, Amount> TransitionsLog<T, LimitLen<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    pub fn forward(
        &mut self,
        meta: &RevMeta,
        c: impl FnOnce(LogMut<T>) -> U,
    ) -> Result<(), AmountErr<T, U, Amount>> {
        if should_pop_transition_at_push(self.log_len(), meta) {
            self.pop_front();
            // TODO:
            // It may be possible to pop more than one entry if other entries are older than meta.log_start as well.
            // A loop here could be more expensive but this can prevent `self.transitions` to reallocate if `c`
            // yields more transitions than the single pop_front here did free.
        }
        self.push_back(|transitions| LimitLen(c(transitions)))
            .map_err(|AmountErr { data, entry, err }| AmountErr {
                data,
                entry: entry.0,
                err,
            })
    }
}
