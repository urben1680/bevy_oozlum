use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{
    should_pop_transition_at_push, LimitLen, OutOfLog, RareData, WithTimestamp, BACKWARD_EXPECT_MSG,
};

#[derive(Debug, Component, Resource)]
pub struct RareTransitionLog<T> {
    transitions: VecDeque<RareData<T>>,
    index: usize,
    skips: usize,
    skips_max: usize,
    len: usize,
}

impl<T> Default for RareTransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> RareTransitionLog<T> {
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            index: 0,
            skips: 0,
            skips_max: 0,
            len: 0,
        }
    }
    pub fn with_capacity(values_capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(values_capacity),
            index: 0,
            skips: 0,
            skips_max: 0,
            len: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.len
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }
    pub fn front(&self) -> RareData<Option<&T>> {
        self.transitions
            .front()
            .map(|entry| RareData {
                data: Some(&entry.data),
                skips_before_value: entry.skips_before_value,
            })
            .unwrap_or_else(|| RareData {
                data: None,
                skips_before_value: self.skips,
            })
    }
    pub fn pop_front(&mut self) -> RareData<Option<T>> {
        self.transitions
            .pop_front()
            .map(|entry| {
                self.len -= entry.skips_before_value;
                RareData {
                    data: Some(entry.data),
                    skips_before_value: entry.skips_before_value + 1, // value itself adds to the len
                }
            })
            .unwrap_or_else(|| {
                self.len = 0;
                RareData {
                    data: None,
                    skips_before_value: self.len,
                }
            })
    }
    pub fn push_back(&mut self, transition: Option<T>) {
        // the order and separation of operations here ensures no underflow occures
        let mut future_len: usize = self.transitions.len() - self.index; // amount of transitions
        future_len += self
            .transitions
            .drain(self.index..)
            .map(|entry| entry.skips_before_value)
            .sum::<usize>(); // plus amount of skips in drain
        future_len += self.skips_max; // plus amount of skips outside of drain
        future_len -= self.skips; // minus amount of skips of either the oldest drained entry or outside drain that is not in the future
        self.len -= future_len;
        self.len += 1; // push_back or increasing skips increases the overall len again
        match transition {
            None => {
                self.skips += 1;
                self.skips_max = self.skips;
            }
            Some(transition) => {
                self.transitions.push_back(RareData {
                    data: transition,
                    skips_before_value: self.skips,
                });
                self.skips = 0;
                self.skips_max = self.skips;
            }
        }
    }
    pub fn backward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            Ok(None)
        } else {
            self.index = self.index.checked_sub(1).ok_or(OutOfLog)?;
            let entry = self
                .transitions
                .get_mut(self.index)
                .expect(BACKWARD_EXPECT_MSG);
            self.skips = entry.skips_before_value;
            Ok(Some(&mut entry.data))
        }
    }
    pub fn forward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if let Some(entry) = self.transitions.get_mut(self.index) {
            if self.skips < entry.skips_before_value {
                self.skips += 1;
                Ok(None)
            } else {
                self.index += 1;
                self.skips = 0;
                Ok(Some(&mut entry.data))
            }
        } else if self.skips < self.skips_max {
            self.skips += 1;
            Ok(None)
        } else {
            Err(OutOfLog)
        }
    }
}

impl<T> RareTransitionLog<WithTimestamp<T>> {
    pub fn forward(&mut self, meta: &RevMeta, transition: Option<T>) {
        if self.front().data.map_or(false, |with_timestamp| {
            // include range().start because this entry instructs how to transition from range().start to range().start - 1
            with_timestamp.logged_at <= meta.range().start
        }) {
            self.pop_front();
        }
        self.push_back(transition.map(|transition| WithTimestamp {
            logged_at: meta.now(),
            data: transition,
        }))
    }
}

impl<T> RareTransitionLog<LimitLen<T>> {
    pub fn forward(&mut self, meta: &RevMeta, transition: Option<T>) {
        if transition.is_some() && should_pop_transition_at_push(self.log_len(), meta) {
            self.pop_front();
        }
        self.push_back(transition.map(|transition| LimitLen(transition)))
    }
}
