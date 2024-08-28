use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{LogIter, NPerFrame, OutOfLog, WithAmount, WithTimestamp};

#[derive(Debug, Default, Clone, Component, Resource)]
pub struct ValueLog<T> {
    /// The log of values, with two partitions:
    /// - Past values in the indices `[0, self.future_index[`
    /// - Future values in the indices `[self.future_index, self.values.len()[`
    ///
    /// The present value is not part of this deque and traversing the log swaps
    /// the present value from before and now while keeping the above partitions.
    values: VecDeque<T>,
    /// The present value, easily accessible to read.
    present: T,
    /// The index of the nearest future value in `self.values`, if there is any.
    future_index: usize,
}

impl<T> From<T> for ValueLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> ValueLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            values: VecDeque::new(),
            present,
            future_index: 0,
        }
    }
    pub fn with_capacity(present: T, capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(capacity),
            present,
            future_index: 0,
        }
    }
    pub fn into_inner(self) -> T {
        self.present
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn capacity(&self) -> usize {
        self.values.capacity()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn reserve(&mut self, additional: usize) {
        self.values.reserve(additional)
    }
    pub fn reserve_exact(&mut self, additional: usize) {
        self.values.reserve_exact(additional)
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve(additional)
    }
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve_exact(additional)
    }
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.values.shrink_to(min_capacity)
    }
    pub fn shrink_to_fit(&mut self) {
        self.values.shrink_to_fit()
    }
    /// Most past value or `None` if the oldest value is considered to be the present value
    pub fn past_end(&self) -> Option<&T> {
        if self.future_index == 0 {
            None
        } else {
            self.values.front()
        }
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.future_index == 0 {
            None
        } else {
            self.future_index -= 1;
            self.values.pop_front()
        }
    }
    pub fn get(&self) -> &T {
        &self.present
    }
    pub fn unlogged_get_mut(&mut self) -> &mut T {
        &mut self.present
    }
    pub fn push_present(&mut self, value: T) {
        self.values.truncate(self.future_index);
        let previous = core::mem::replace(&mut self.present, value);
        self.values.push_back(previous);
        self.future_index += 1;
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.values.drain(self.future_index..)
    }
    pub fn clear(&mut self, present: T) {
        self.values.clear();
        self.present = present;
        self.future_index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        // from:
        //  values:        [1, 2, 4]
        //  present:       3
        //  future_index:  2
        // to:
        //  values:        [1, 3, 4]
        //  present:       2
        //  future_index:  1

        self.future_index = self.future_index.checked_sub(1).ok_or(OutOfLog)?;
        let now_future = self.values.get_mut(self.future_index).expect("todo");
        core::mem::swap(&mut self.present, now_future);
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        // from:
        //  values:        [1, 3, 4]
        //  present:       2
        //  future_index:  1
        // to:
        //  values:        [1, 2, 4]
        //  present:       3
        //  future_index:  2

        if self.future_index == self.values.len() {
            return Err(OutOfLog);
        }
        let now_future = self.values.get_mut(self.future_index).expect("todo");
        core::mem::swap(&mut self.present, now_future);
        self.future_index += 1;
        Ok(())
    }
}

impl<T> ValueLog<WithTimestamp<T>> {
    pub fn pop_past_by_timestamp(&mut self, meta: &RevMeta) -> Option<WithTimestamp<T>> {
        if self.past_end()?.logged_at.0 < meta.range().start {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_timestamp(&mut self, meta: &RevMeta) -> impl LogIter<WithTimestamp<T>> {
        let partition_point = self
            .values
            .partition_point(|entry| entry.logged_at.0 < meta.range().start);
        self.future_index -= partition_point;
        self.values.drain(..partition_point)
    }
}

impl<T, Amount: Copy> ValueLog<WithAmount<WithTimestamp<T>, Amount>> {
    pub(crate) fn pop_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<WithAmount<WithTimestamp<T>, Amount>> {
        if self.past_end()?.entry.logged_at.0 < meta.range().start {
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
            .values
            .partition_point(|entry| entry.entry.logged_at.0 < meta.range().start);
        self.future_index -= partition_point;
        self.values.drain(..partition_point)
    }
}

impl<const N: usize, T> ValueLog<NPerFrame<N, T>> {
    pub fn pop_past_by_len(&mut self, meta: &RevMeta) -> Option<NPerFrame<N, T>> {
        if self.future_index > meta.past_len() * N {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, meta: &RevMeta) -> impl LogIter<NPerFrame<N, T>> {
        let excessive = self.future_index.saturating_sub(meta.past_len() * N);
        self.future_index -= excessive;
        self.values.drain(..excessive)
    }
}

impl<const N: usize, T, Amount: Copy> ValueLog<WithAmount<NPerFrame<N, T>, Amount>> {
    pub(crate) fn pop_past_by_len(
        &mut self,
        meta: &RevMeta,
    ) -> Option<WithAmount<NPerFrame<N, T>, Amount>> {
        if self.future_index > meta.past_len() * N {
            self.pop_past()
        } else {
            None
        }
    }
    pub(crate) fn drain_past_by_len(
        &mut self,
        meta: &RevMeta,
    ) -> impl LogIter<WithAmount<NPerFrame<N, T>, Amount>> {
        let excessive = self.future_index.saturating_sub(meta.past_len() * N);
        self.future_index -= excessive;
        self.values.drain(..excessive)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use crate::log::OnePerFrame;

    use super::*;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [ValueLog<WithTimestamp<usize>>; 2],
        one_per_frame: [ValueLog<OnePerFrame<usize>>; 2],
    }

    impl MetaAndLogs {
        fn new(present: usize, max_len: Option<NonZeroUsize>) -> Self {
            let meta = RevMeta::new(max_len, 0, false);
            let with_timestamp =
                ValueLog::<WithTimestamp<usize>>::from(meta.with_timestamp(present));
            let one_per_frame = OnePerFrame::<usize>::from(present);
            let one_per_frame = ValueLog::<OnePerFrame<usize>>::from(one_per_frame);
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: [with_timestamp.clone(), with_timestamp],
                one_per_frame: [one_per_frame.clone(), one_per_frame],
            }
        }
        fn forward(&mut self, value: usize, expected_log_len: usize) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update_inner();

            self.with_timestamp[0].push_present(self.meta.with_timestamp(value));
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].pop_past_by_timestamp(&self.meta);
            assert_eq!(
                self.with_timestamp[0].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );
            assert_eq!(
                self.with_timestamp[0].get().data,
                value,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );

            self.with_timestamp[1].push_present(self.meta.with_timestamp(value));
            let middle = self.with_timestamp[1].clone();
            let _ = self.with_timestamp[1].drain_past_by_timestamp(&self.meta);
            assert_eq!(
                self.with_timestamp[1].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );
            assert_eq!(
                self.with_timestamp[1].get().data,
                value,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );

            self.one_per_frame[0].push_present(value.into());
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(&self.meta);
            assert_eq!(
                self.one_per_frame[0].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );
            assert_eq!(
                self.one_per_frame[0].get().0,
                value,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );

            self.one_per_frame[1].push_present(value.into());
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(&self.meta);
            assert_eq!(
                self.one_per_frame[1].len(),
                expected_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
            assert_eq!(
                self.one_per_frame[1].get().0,
                value,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
        }
        fn backward_log(&mut self, expected_value: Result<usize, OutOfLog>) {
            let previous = self.clone();

            match expected_value {
                Ok(expected_value) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update_inner();

                    let is_ok = self.with_timestamp[0].backward_log().is_ok();
                    let value = self.with_timestamp[0].get().data;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );
                    assert_eq!(
                        value, expected_value,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let is_ok = self.with_timestamp[1].backward_log().is_ok();
                    let value = self.with_timestamp[1].get().data;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );
                    assert_eq!(
                        value, expected_value,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let is_ok = self.one_per_frame[0].backward_log().is_ok();
                    let value = self.one_per_frame[0].get().0;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );
                    assert_eq!(
                        value, expected_value,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let is_ok = self.one_per_frame[1].backward_log().is_ok();
                    let value = self.one_per_frame[1].get().0;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                    assert_eq!(
                        value, expected_value,
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
        fn forward_log(&mut self, expected_value: Result<usize, OutOfLog>) {
            let previous = self.clone();
            match expected_value {
                Ok(expected_value) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update_inner();

                    let is_ok = self.with_timestamp[0].forward_log().is_ok();
                    let value = self.with_timestamp[0].get().data;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );
                    assert_eq!(
                        value, expected_value,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let is_ok = self.with_timestamp[1].forward_log().is_ok();
                    let value = self.with_timestamp[1].get().data;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );
                    assert_eq!(
                        value, expected_value,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let is_ok = self.one_per_frame[0].forward_log().is_ok();
                    let value = self.one_per_frame[0].get().0;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );
                    assert_eq!(
                        value, expected_value,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let is_ok = self.one_per_frame[1].forward_log().is_ok();
                    let value = self.one_per_frame[1].get().0;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                    assert_eq!(
                        value, expected_value,
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
}
