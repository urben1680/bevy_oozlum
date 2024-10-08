use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::{
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::error,
};

use super::{LogIter, LoggedAt, OutOfLog, PackedTime, INDEX_OOB};

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionLog<T> {
    transitions: VecDeque<T>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::TransitionLog;

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for TransitionLog<T> {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<T>>, usize);
        type De = (WithCapacityWrapper<VecDeque<T>>, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.transitions), self.index)
        }
        fn from_with_capacity(with_capacity: Self::De) -> Self {
            Self {
                transitions: with_capacity.0 .0,
                index: with_capacity.1,
            }
        }
    }

    impl<T> LoglessWithCapacity for TransitionLog<T> {
        type Se<'se> = usize where T: 'se;
        type De = usize;
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            self.transitions.capacity()
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
    }
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
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        let transitions_len = self.transitions.len();
        if let Some(transition) = self.transitions.get_mut(index) {
            self.index = index;
            return Ok(transition);
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct TransitionLogDebug {
            transitions_len: usize,
            index: usize,
        }

        let debug_struct = TransitionLogDebug {
            transitions_len,
            index: self.index,
        };

        error!("{INDEX_OOB}, {debug_struct:#?}");
        Err(OutOfLog)
    }
    pub fn forward_log(&mut self) -> Result<&mut T, OutOfLog> {
        self.transitions
            .get_mut(self.index)
            .inspect(|_| self.index += 1)
            .ok_or(OutOfLog)
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        if self.index > max_past_len {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
        let excessive = self.index.saturating_sub(max_past_len);
        self.index -= excessive;
        self.transitions.drain(..excessive)
    }
}

impl<T: LoggedAt> TransitionLog<T> {
    pub fn pop_past_by_timestamp(&mut self, log_start: usize) -> Option<T> {
        if self.past_end()?.logged_at() <= log_start {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_timestamp(&mut self, log_start: usize) -> impl LogIter<T> {
        let partition_point = self
            .transitions
            .partition_point(|entry| entry.logged_at() <= log_start);
        self.index -= partition_point;
        self.transitions.drain(..partition_point)
    }
    pub fn reduce_timestamps(&mut self, by: usize) -> impl LogIter<T> {
        let reduced_at = self
            .transitions
            .range_mut(..self.index)
            .position(|with_timestamp| {
                with_timestamp
                    .logged_at()
                    .checked_sub(by)
                    .inspect(|reduced| {
                        with_timestamp.set_logged_at(PackedTime::from_internal(*reduced))
                    })
                    .is_some()
            })
            .unwrap_or(self.index);
        let mut iter = self.transitions.range_mut(reduced_at..);
        if reduced_at == self.index {
            if let Some(with_timestamp) = iter.next() {
                let logged_at = with_timestamp.logged_at();
                let logged_at = match logged_at.checked_sub(by) {
                    Some(reduced) => PackedTime::from_internal(reduced),
                    None => panic!(
                        "future transition was logged at {logged_at} which cannot be reduced by {by}"
                    ),
                };
                with_timestamp.set_logged_at(logged_at);
            }
        }
        for with_timestamp in iter {
            let logged_at = with_timestamp.logged_at();
            with_timestamp.set_logged_at(PackedTime::from_internal(logged_at - by));
        }
        self.index -= reduced_at;
        self.transitions.drain(..reduced_at)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use super::*;

    use crate::{log::WithLoggedAt, meta::RevMeta};

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [TransitionLog<WithLoggedAt<usize>>; 2],
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
            self.meta.update();

            self.with_timestamp[0].pop_past_by_timestamp(self.meta.log_range().start);
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].push_present(self.meta.with_logged_at(transition));
            assert_eq!(
                self.with_timestamp[0].len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );

            let _ = self.with_timestamp[1].drain_past_by_timestamp(self.meta.log_range().start);
            let middle = self.with_timestamp[1].clone();
            self.with_timestamp[1].push_present(self.meta.with_logged_at(transition));
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
            self.one_per_frame[0].pop_past_by_len(self.meta.past_len());
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
            let _ = self.one_per_frame[1].drain_past_by_len(self.meta.past_len());
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
                    self.meta.update();

                    assert_eq!(
                        self.with_timestamp[0].backward_log().map(|x| x.value),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    assert_eq!(
                        self.with_timestamp[1].backward_log().map(|x| x.value),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    assert_eq!(
                        self.one_per_frame[0].backward_log().cloned(),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    assert_eq!(
                        self.one_per_frame[1].backward_log().cloned(),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(OutOfLog) => {
                    assert_eq!(
                        self.with_timestamp[0].backward_log().cloned(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].backward_log().cloned(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].backward_log().cloned(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].backward_log().cloned(),
                        Err(OutOfLog),
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
                    self.meta.update();

                    assert_eq!(
                        self.with_timestamp[0].forward_log().map(|x| x.value),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    assert_eq!(
                        self.with_timestamp[1].forward_log().map(|x| x.value),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    assert_eq!(
                        self.one_per_frame[0].forward_log().cloned(),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    assert_eq!(
                        self.one_per_frame[1].forward_log().cloned(),
                        expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(OutOfLog) => {
                    assert_eq!(
                        self.with_timestamp[0].forward_log().cloned(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].forward_log().cloned(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].forward_log().cloned(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].forward_log().cloned(),
                        Err(OutOfLog),
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

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<TransitionLog<WithLoggedAt<usize>>>();
    }
}
