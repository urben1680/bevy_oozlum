use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{LogIter, OutOfLog, WithAmount, WithTimestamp, BACKWARD_EXPECT_MSG};

#[derive(Debug, Component, Clone, Resource)]
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
    pub fn past_end(&self) -> Option<&T> {
        self.transitions.front()
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            // if the current log position is at the past end, transitions.front() is not a past value but a future value
            return None;
        }
        self.transitions.pop_front().inspect(|_| self.index -= 1)
    }
    pub fn push_present(&mut self, transition: T) {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        self.index = self.transitions.len();
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.transitions.drain(self.index..)
    }
    pub fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
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
    pub fn pop_past_by_len(&mut self, meta: &RevMeta, push_per_frame: usize) -> Option<T> {
        if self.index > meta.past_len() * push_per_frame {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, meta: &RevMeta, push_per_frame: usize) -> impl LogIter<T> {
        let excessive = self.index.saturating_sub(meta.past_len() * push_per_frame);
        self.index -= excessive;
        self.transitions.drain(..excessive)
    }
}

impl<T> TransitionLog<WithTimestamp<T>> {
    pub fn pop_past_by_timestamp(&mut self, meta: &RevMeta) -> Option<WithTimestamp<T>> {
        if self.past_end().map_or(false, |with_timestamp| {
            // include range().start because this entry instructs how to transition from range().start to range().start - 1
            with_timestamp.logged_at.0 <= meta.range().start
        }) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_timestamp(&mut self, meta: &RevMeta) -> impl LogIter<WithTimestamp<T>> {
        let partition_point = self
            .transitions
            .partition_point(|entry| entry.logged_at.0 <= meta.range().start);
        self.index -= partition_point;
        self.transitions.drain(..partition_point)
    }
}

impl<T, Amount: Copy> TransitionLog<WithAmount<WithTimestamp<T>, Amount>> {
    pub(crate) fn pop_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<WithAmount<WithTimestamp<T>, Amount>> {
        if self.past_end().map_or(false, |with_timestamp| {
            // include range().start because this entry instructs how to transition from range().start to range().start - 1
            with_timestamp.entry.logged_at.0 <= meta.range().start
        }) {
            self.pop_past()
        } else {
            None
        }
    }
    pub(crate) fn drain_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> impl LogIter<WithAmount<WithTimestamp<T>, Amount>> {
        let partition_point = self
            .transitions
            .partition_point(|entry| entry.entry.logged_at.0 <= meta.range().start);
        self.index -= partition_point;
        self.transitions.drain(..partition_point)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use super::*;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [TransitionLog<WithTimestamp<usize>>; 2],
        one_per_frame: [TransitionLog<usize>; 2],
    }

    impl MetaAndLogs {
        fn new(max_len: Option<NonZeroUsize>) -> Self {
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: Default::default(),
                one_per_frame: Default::default(),
            }
        }
        fn forward(&mut self, transition: usize, expected_len: usize) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update_inner();

            self.with_timestamp[0].pop_past_by_timestamp(&self.meta);
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].push_present(self.meta.with_timestamp(transition));
            assert_eq!(
                self.with_timestamp[0].len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );

            let _ = self.with_timestamp[1].drain_past_by_timestamp(&self.meta);
            let middle = self.with_timestamp[1].clone();
            self.with_timestamp[1].push_present(self.meta.with_timestamp(transition));
            assert_eq!(
                self.with_timestamp[1].len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );

            self.one_per_frame[0].push_present(transition.into());
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(&self.meta, 1);
            assert_eq!(
                self.one_per_frame[0].len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );

            self.one_per_frame[1].push_present(transition.into());
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(&self.meta, 1);
            assert_eq!(
                self.one_per_frame[1].len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
        }
        fn backward_log(&mut self, expected_transition: Result<usize, OutOfLog>) {
            let previous = self.clone();
            match expected_transition {
                Ok(_) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update_inner();

                    let transition = self.with_timestamp[0]
                        .backward_log()
                        .map(|entry| entry.data);
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let transition = self.with_timestamp[1]
                        .backward_log()
                        .map(|entry| entry.data);
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let transition = self.one_per_frame[0].backward_log().cloned();
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let transition = self.one_per_frame[1].backward_log().cloned();
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                }
                Err(OutOfLog) => {
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
        fn forward_log(&mut self, expected_transition: Result<usize, OutOfLog>) {
            let previous = self.clone();
            match expected_transition {
                Ok(_) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update_inner();

                    let transition = self.with_timestamp[0].forward_log().map(|entry| entry.data);
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let transition = self.with_timestamp[1].forward_log().map(|entry| entry.data);
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let transition = self.one_per_frame[0].forward_log().cloned();
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let transition = self.one_per_frame[1].forward_log().cloned();
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                }
                Err(OutOfLog) => {
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
        let mut meta_and_logs = MetaAndLogs::new(NonZeroUsize::new(3));

        meta_and_logs.forward(1, 1);
        meta_and_logs.forward(2, 2);
        // pop_front called internally
        meta_and_logs.forward(3, 2);

        meta_and_logs.backward_log(Ok(3));
        meta_and_logs.backward_log(Ok(2));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err(OutOfLog));

        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Ok(3));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(3));
        meta_and_logs.backward_log(Ok(2));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(4, 1);
    }
}
