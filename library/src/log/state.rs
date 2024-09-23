use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::{reflect::Reflect, utils::tracing::error};

use crate::log::INDEX_OOB;

use super::{LoggedAt, LogIter, OutOfLog, PackedTime};

#[derive(Debug, Default, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StateLog<T> {
    /// The log of states, with two partitions:
    /// - Past states in the indices `[0, self.index[`
    /// - Future states in the indices `[self.index, self.states.len()[`
    ///
    /// The present state is not part of this deque and traversing the log swaps
    /// the present state from before and now while keeping the above partitions.
    states: VecDeque<T>,
    /// The present state, easily accessible to read.
    present: T,
    /// The index of the nearest future state in `self.states`, if there is any.
    index: usize,
}

#[derive(Debug)]
#[allow(dead_code)]
struct StateLogDebug {
    states_len: usize,
    index: usize,
}

impl<T> From<T> for StateLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> StateLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present,
            index: 0,
        }
    }
    pub fn with_capacity(present: T, capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(capacity),
            present,
            index: 0,
        }
    }
    pub fn into_inner(self) -> T {
        self.present
    }
    pub fn len(&self) -> usize {
        self.states.len()
    }
    pub fn capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn shrink_to_fit(&mut self) {
        self.states.shrink_to_fit()
    }
    pub fn get(&self) -> &T {
        &self.present
    }
    pub fn unlogged_get_mut(&mut self) -> &mut T {
        &mut self.present
    }
    /// Most past state or `None` if the oldest state is considered to be the present state
    pub fn past_end(&self) -> Option<&T> {
        if self.index == 0 {
            return None;
        }
        self.states.front()
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        self.states.pop_front()
    }
    pub fn push_present(&mut self, state: T) {
        self.states.truncate(self.index);
        let previous = core::mem::replace(&mut self.present, state);
        self.states.push_back(previous);
        self.index += 1;
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.states.drain(self.index..)
    }
    pub fn clear(&mut self) {
        self.states.clear();
        self.index = 0;
    }
    pub fn clear_with(&mut self, present: T) {
        self.states.clear();
        self.present = present;
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        // from:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2
        // to:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1

        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        if let Some(now_future) = self.states.get_mut(index) {
            self.index = index;
            core::mem::swap(&mut self.present, now_future);
            return Ok(());
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct StateLogDebug {
            states_len: usize,
            index: usize,
        }

        let debug_struct = StateLogDebug {
            states_len: self.states.len(),
            index: self.index,
        };

        error!("{INDEX_OOB}, {debug_struct:#?}");
        Err(OutOfLog)
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        // from:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1
        // to:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2

        if let Some(now_future) = self.states.get_mut(self.index) {
            core::mem::swap(&mut self.present, now_future);
            self.index += 1;
            return Ok(());
        }

        if self.index != self.states.len() {
            let debug_struct = StateLogDebug {
                states_len: self.states.len(),
                index: self.index,
            };
            error!("{INDEX_OOB}, {debug_struct:#?}");
        }

        Err(OutOfLog)
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
        self.states.drain(..excessive)
    }
}

impl<T: LoggedAt> StateLog<T> {
    pub fn pop_past_by_timestamp(&mut self, log_start: usize) -> Option<T> {
        if self.past_end()?.logged_at() < log_start {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_timestamp(&mut self, log_start: usize) -> impl LogIter<T> {
        let partition_point = self
            .states
            .partition_point(|entry| entry.logged_at() < log_start);
        self.index -= partition_point;
        self.states.drain(..partition_point)
    }
    pub fn reduce_timestamps(&mut self, by: usize) -> impl LogIter<T> {
        let reduced_at = self
            .states
            .range_mut(..self.index)
            .position(|with_timestamp| {
                with_timestamp
                    .logged_at()
                    .checked_sub(by)
                    .inspect(|reduced| {
                        with_timestamp.set_logged_at(PackedTime::from_internal(*reduced))
                    })
                    .is_some()
            });
        let logged_at = match reduced_at {
            Some(_) => PackedTime::from_internal(self.present.logged_at() - by),
            None => {
                let logged_at = self.present.logged_at();
                match logged_at.checked_sub(by) {
                    Some(reduced) => PackedTime::from_internal(reduced),
                    None => panic!(
                        "present state was logged at {logged_at} which cannot be reduced by {by}"
                    ),
                }
            }
        };
        self.present.set_logged_at(logged_at); 
        let reduced_at = reduced_at.unwrap_or(self.index);
        for with_timestamp in self.states.range_mut(reduced_at..) {
            let logged_at = with_timestamp.logged_at();
            with_timestamp.set_logged_at(PackedTime::from_internal(logged_at - by));
        }
        self.index -= reduced_at;
        self.states.drain(..reduced_at)
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
        with_timestamp: [StateLog<WithLoggedAt<usize>>; 2],
        one_per_frame: [StateLog<usize>; 2],
    }

    impl MetaAndLogs {
        fn new(present: usize, max_len: Option<NonZeroUsize>) -> Self {
            let meta = RevMeta::new(max_len, 0, false);
            let with_timestamp =
                StateLog::<WithLoggedAt<usize>>::from(meta.with_timestamp(present));
            let one_per_frame = StateLog::from(present);
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: [with_timestamp.clone(), with_timestamp],
                one_per_frame: [one_per_frame.clone(), one_per_frame],
            }
        }
        fn forward(&mut self, state: usize, expected_log_len: usize) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update();

            self.with_timestamp[0].push_present(self.meta.with_timestamp(state));
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].pop_past_by_timestamp(self.meta.log_range().start);
            assert_eq!(
                self.with_timestamp[0].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );
            assert_eq!(
                self.with_timestamp[0].get().value,
                state,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );

            self.with_timestamp[1].push_present(self.meta.with_timestamp(state));
            let middle = self.with_timestamp[1].clone();
            let _ = self.with_timestamp[1].drain_past_by_timestamp(self.meta.log_range().start);
            assert_eq!(
                self.with_timestamp[1].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );
            assert_eq!(
                self.with_timestamp[1].get().value,
                state,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );

            self.one_per_frame[0].push_present(state.into());
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(self.meta.past_len());
            assert_eq!(
                self.one_per_frame[0].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );
            assert_eq!(
                *self.one_per_frame[0].get(),
                state,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );

            self.one_per_frame[1].push_present(state.into());
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(self.meta.past_len());
            assert_eq!(
                self.one_per_frame[1].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
            assert_eq!(
                *self.one_per_frame[1].get(),
                state,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
        }
        fn backward_log(&mut self, expected_state: Result<usize, OutOfLog>) {
            let previous = self.clone();

            match expected_state {
                Ok(expected_state) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    assert_eq!(
                        self.with_timestamp[0].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[0].get().value,
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    assert_eq!(
                        self.with_timestamp[1].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.with_timestamp[1].get().value,
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    assert_eq!(
                        self.one_per_frame[0].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        *self.one_per_frame[0].get(),
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    assert_eq!(
                        self.one_per_frame[1].backward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                    assert_eq!(
                        *self.one_per_frame[1].get(),
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(OutOfLog) => {
                    assert_eq!(
                        self.with_timestamp[0].backward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].backward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].backward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].backward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
            }
        }
        fn forward_log(&mut self, expected_state: Result<usize, OutOfLog>) {
            let previous = self.clone();
            match expected_state {
                Ok(expected_state) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    assert_eq!(
                        self.with_timestamp[0].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[0].get().value,
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    assert_eq!(
                        self.with_timestamp[1].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.with_timestamp[1].get().value,
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    assert_eq!(
                        self.one_per_frame[0].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        *self.one_per_frame[0].get(),
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    assert_eq!(
                        self.one_per_frame[1].forward_log(),
                        Ok(()),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                    assert_eq!(
                        *self.one_per_frame[1].get(),
                        expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(OutOfLog) => {
                    assert_eq!(
                        self.with_timestamp[0].forward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].forward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].forward_log(),
                        Err(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].forward_log(),
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
        let mut meta_and_logs = MetaAndLogs::new(0, NonZeroUsize::new(3));

        meta_and_logs.forward(1, 1);
        meta_and_logs.forward(2, 2);
        // pop_front called internally
        meta_and_logs.forward(3, 2);

        meta_and_logs.backward_log(Ok(2));
        meta_and_logs.backward_log(Ok(1));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err(OutOfLog));

        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Ok(3));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(2));
        meta_and_logs.backward_log(Ok(1));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(4, 1);
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<StateLog<WithLoggedAt<usize>>>();
    }
}
