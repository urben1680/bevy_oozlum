use core::fmt::Debug;
use std::{
    collections::{vec_deque::Drain, TryReserveError, VecDeque},
    usize,
};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{
    should_pop_transition_at_push, LimitLen, OutOfLog, WithTimestamp, BACKWARD_EXPECT_MSG,
};

#[derive(Debug, Component, Resource)]
pub struct TransitionLog<T> {
    transitions: VecDeque<T>,
    index: usize,
}

impl<T> Default for TransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> TransitionLog<T> {
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(capacity),
            index: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.transitions.len()
    }
    pub fn capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }
    pub fn front(&self) -> Option<&T> {
        self.transitions.front()
    }
    pub fn pop_front(&mut self) -> Option<T> {
        self.transitions.pop_front()
    }
    pub fn push_back(&mut self, transition: T) {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        self.index = self.transitions.len();
    }
    pub fn drain_future(&mut self) -> Drain<T> {
        self.transitions.drain(self.index..)
    }
    pub fn backward_log(&mut self) -> Result<&mut T, OutOfLog> {
        self.index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        Ok(self
            .transitions
            .get_mut(self.index)
            .expect(BACKWARD_EXPECT_MSG))
    }
    pub fn forward_log(&mut self) -> Result<&mut T, OutOfLog> {
        self.transitions
            .get_mut(self.index)
            .inspect(|_| self.index += 1)
            .ok_or(OutOfLog)
    }
}

impl<T> TransitionLog<WithTimestamp<T>> {
    pub fn forward(&mut self, meta: &RevMeta, transition: T) {
        if self.front().map_or(false, |with_timestamp| {
            // include range().start because this entry instructs how to transition from range().start to range().start - 1
            with_timestamp.logged_at <= meta.range().start
        }) {
            self.pop_front();
        }
        self.push_back(WithTimestamp {
            logged_at: meta.now(),
            data: transition,
        });
    }
    /// When [`RevMeta::max_len`] is reduced, [`Self::forward`] will not reduce to the new len but will only prevent reallocations.
    ///
    /// If one wants to force older entries to be dropped, for example to call [`Self::shrink_to_fit`] afterwards, this method can be used.
    pub fn truncate_to_fit(&mut self, meta: &RevMeta) {
        let result = self
            .transitions
            .binary_search_by_key(&meta.range().start, |entry| entry.logged_at);
        match result {
            Ok(index) => self.transitions.drain(..=index),
            Err(index) => self.transitions.drain(..index),
        };
    }
}

impl<T> TransitionLog<LimitLen<T>> {
    pub fn forward(&mut self, meta: &RevMeta, transition: T) {
        if should_pop_transition_at_push(self.len(), meta) {
            self.pop_front();
        }
        self.push_back(LimitLen(transition));
    }
    /* todo: this is sensitive to be called in a specific order to forward/backward_log/forward_log, make this failure safe or warn in the docs
    /// When [`RevMeta::max_len`] is reduced, [`Self::forward`] will not reduce to the new len but will only prevent reallocations.
    ///
    /// If one wants to force older entries to be dropped, for example to call [`Self::shrink_to_fit`] afterwards, this method can be used.
    pub fn truncate_to_fit(&mut self, meta: &RevMeta) {
        let past_len = meta.now() - meta.range().start;
        if let Some(excessive_len) = self.index.checked_sub(past_len) {
            self.transitions.drain(..excessive_len);
        }
    }
    */
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use super::*;

    struct MetaAndLogs {
        meta: RevMeta,
        additions_with_timestamp: TransitionLog<WithTimestamp<usize>>,
        additions_limit_len: TransitionLog<LimitLen<usize>>,
        data: usize,
    }

    impl MetaAndLogs {
        fn new(max_len: Option<NonZeroUsize>) -> Self {
            Self {
                meta: RevMeta::new(max_len, 0, false),
                additions_with_timestamp: Default::default(),
                additions_limit_len: Default::default(),
                data: 0b1,
            }
        }
        fn forward(&mut self, expected_data: usize, expected_len: usize) {
            self.meta.queue_forward();
            self.meta.update();
            let old = self.data;
            self.data *= 2;
            assert_eq!(self.data, expected_data);
            let transition = self.data - old;
            self.additions_with_timestamp
                .forward(&self.meta, transition);
            self.additions_limit_len.forward(&self.meta, transition);
            assert_eq!(self.additions_with_timestamp.len(), expected_len);
            assert_eq!(self.additions_limit_len.len(), expected_len);
        }
        fn backward_log(&mut self, expected_data: Result<usize, OutOfLog>) {
            match expected_data {
                Ok(expected_data) => {
                    assert!(self.meta.queue_log(self.meta.now() - 1).is_ok());
                    self.meta.update();
                    let addition1 = self.additions_with_timestamp.backward_log().unwrap().data;
                    let addition2 = self.additions_limit_len.backward_log().unwrap().0;
                    assert_eq!(addition1, addition2);
                    self.data -= addition1;
                    assert_eq!(self.data, expected_data);
                }
                Err(OutOfLog) => {
                    assert!(self.meta.queue_log(self.meta.now() - 1).is_err());
                    assert_eq!(self.additions_with_timestamp.backward_log(), Err(OutOfLog));
                    assert_eq!(self.additions_limit_len.backward_log(), Err(OutOfLog));
                }
            }
        }
        fn forward_log(&mut self, expected_data: Result<usize, OutOfLog>) {
            match expected_data {
                Ok(expected_data) => {
                    assert!(self.meta.queue_log(self.meta.now() + 1).is_ok());
                    self.meta.update();
                    let addition1 = self.additions_with_timestamp.forward_log().unwrap().data;
                    let addition2 = self.additions_limit_len.forward_log().unwrap().0;
                    assert_eq!(addition1, addition2);
                    self.data += addition1;
                    assert_eq!(self.data, expected_data);
                }
                Err(OutOfLog) => {
                    assert!(self.meta.queue_log(self.meta.now() + 1).is_err());
                    assert_eq!(self.additions_with_timestamp.forward_log(), Err(OutOfLog));
                    assert_eq!(self.additions_limit_len.forward_log(), Err(OutOfLog));
                }
            }
        }
    }

    #[test]
    fn test() {
        let mut meta_and_logs = MetaAndLogs::new(NonZeroUsize::new(3));

        meta_and_logs.forward(2, 1);
        meta_and_logs.forward(4, 2);
        // pop_front called internally
        meta_and_logs.forward(8, 2);

        meta_and_logs.backward_log(Ok(4));
        meta_and_logs.backward_log(Ok(2));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err(OutOfLog));

        meta_and_logs.forward_log(Ok(4));
        meta_and_logs.forward_log(Ok(8));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(4));
        meta_and_logs.backward_log(Ok(2));
        // all entried are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(4, 1);
    }
}
