use std::{
    collections::{
        vec_deque::{Drain, IterMut},
        TryReserveError, VecDeque,
    },
    fmt::Debug,
};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{
    amount_to_usize, AmountErr, DataEntry, LimitLen, LogMut, OutOfLog, RareData, RareTransitionLog,
    WithAmount, WithTimestamp,
};

#[derive(Debug, Component, Resource)]
pub struct RareTransitionsLog<T, U = (), Amount = usize>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    amounts: RareTransitionLog<WithAmount<U, Amount>>,
    transitions: VecDeque<T>,
    index: usize,
}

impl<T, U, Amount> Default for RareTransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U, Amount> RareTransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
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
    pub fn front(&self) -> RareData<Option<&WithAmount<U, Amount>>> {
        self.amounts.front()
    }
    pub fn pop_front(&mut self) -> RareData<Option<DataEntry<Drain<T>, U>>> {
        self.amounts.pop_front().map(|with_amount| {
            let amount = amount_to_usize(with_amount.amount);
            DataEntry {
                data: self.transitions.drain(..amount),
                entry: with_amount.entry,
            }
        })
    }
    pub fn push_back(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> U,
    ) -> Result<Option<U>, AmountErr<T, U, Amount>> {
        let previous_len = self.transitions.len();
        let entry = c(LogMut(&mut self.transitions));
        let new_amount = self.transitions.len() - previous_len;
        if new_amount == 0 {
            self.amounts.push_back(None);
            return Ok(Some(entry));
        }
        match new_amount.try_into() {
            Ok(amount) => {
                self.amounts.push_back(Some(WithAmount { entry, amount }));
                Ok(None)
            }
            Err(err) => Err(AmountErr {
                entry,
                data: self.transitions.drain(previous_len..),
                err,
            }),
        }
    }
    pub fn backward_log(&mut self) -> Result<Option<DataEntry<IterMut<T>, &U>>, OutOfLog> {
        Ok(self.amounts.backward_log()?.map(|with_amount| {
            let old_index = self.index;
            let amount = amount_to_usize(with_amount.amount.clone());
            self.index -= amount;
            let iter = self.transitions.range_mut(self.index..old_index);
            DataEntry {
                data: iter,
                entry: &with_amount.entry,
            }
        }))
    }
    pub fn forward_log(&mut self) -> Result<Option<DataEntry<IterMut<T>, &U>>, OutOfLog> {
        Ok(self.amounts.forward_log()?.map(|with_amount| {
            let amount = amount_to_usize(with_amount.amount.clone());
            let old_index = self.index;
            self.index += amount;
            let iter = self.transitions.range_mut(old_index..self.index);
            DataEntry {
                data: iter,
                entry: &with_amount.entry,
            }
        }))
    }
}

impl<T, U, Amount> RareTransitionsLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    pub fn forward(
        &mut self,
        meta: &RevMeta,
        c: impl FnOnce(LogMut<T>) -> U,
    ) -> Result<Option<WithTimestamp<U>>, AmountErr<T, WithTimestamp<U>, Amount>> {
        if self.front().data.map_or(false, |with_amount| {
            // include range().start because this entry instructs how to transition from range().start to range().start - 1
            with_amount.entry.logged_at <= meta.range().start
        }) {
            self.pop_front();
            // TODO:
            // It may be possible to pop more than one entry if other entries are older than meta.log_start as well.
            // A loop here could be more expensive but this can prevent `self.transitions` to reallocate if `c`
            // yields more transitions than the single pop_front here did free.
        }
        self.push_back(|transitions| WithTimestamp {
            logged_at: meta.now(),
            data: c(transitions),
        })
    }
}

impl<T, U, Amount> RareTransitionsLog<T, LimitLen<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Clone,
{
    pub fn forward(
        &mut self,
        meta: &RevMeta,
        c: impl FnOnce(LogMut<T>) -> U,
    ) -> Result<Option<U>, AmountErr<T, U, Amount>> {
        if self.log_len() <= meta.range().len() {
            self.pop_front();
            // TODO:
            // It may be possible to pop more than one entry if other entries are older than meta.log_start as well.
            // A loop here could be more expensive but this can prevent `self.transitions` to reallocate if `c`
            // yields more transitions than the single pop_front here did free.
        }
        match self.push_back(|transitions| LimitLen(c(transitions))) {
            Ok(None) => Ok(None),
            Ok(Some(LimitLen(entry))) => Ok(Some(entry)),
            Err(AmountErr { data, entry, err }) => Err(AmountErr {
                data,
                entry: entry.0,
                err,
            }),
        }
    }
}

impl<T> RareData<Option<T>> {
    // todo: nur eine nutzung, impl rauswerfen oder für rare_values in oberes modul verschieben
    fn map<U>(self, f: impl FnOnce(T) -> U) -> RareData<Option<U>> {
        RareData {
            data: self.data.map(f),
            skips_before_value: self.skips_before_value,
        }
    }
}
