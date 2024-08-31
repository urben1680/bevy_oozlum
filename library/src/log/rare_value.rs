use core::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::{TryReserveError, VecDeque},
};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{LogIter, OutOfLog, Packed, RareData, WithAmount, WithTimestamp, BACKWARD_EXPECT_MSG};

#[derive(Debug, Clone, Component, Resource)]
pub struct RareValueLog<T> {
    /// RareData.skips represents the number of None pushes after the value in the struct
    values: VecDeque<RareData<T>>,
    present: RareData<T>,
    index: usize,
    skips: usize,
    len: usize,
}

impl<T> From<T> for RareValueLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> RareValueLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            values: VecDeque::new(),
            present: RareData {
                data: present,
                skips: Packed(0),
            },
            index: 0,
            skips: 0,
            len: 0,
        }
    }

    pub fn with_capacity(present: T, values_capacity: usize) -> Self {
        Self {
            values: VecDeque::with_capacity(values_capacity),
            present: RareData {
                data: present,
                skips: Packed(0),
            },
            index: 0,
            skips: 0,
            len: 0,
        }
    }
    pub fn into_inner(self) -> T {
        self.present.data
    }
    pub fn log_len(&self) -> usize {
        self.len
    }
    pub fn values_len(&self) -> usize {
        self.values.len()
    }
    pub fn values_capacity(&self) -> usize {
        self.values.capacity()
    }
    pub fn values_is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn values_reserve(&mut self, additional: usize) {
        self.values.reserve(additional)
    }
    pub fn values_reserve_exact(&mut self, additional: usize) {
        self.values.reserve_exact(additional)
    }
    pub fn values_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve(additional)
    }
    pub fn values_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.values.try_reserve_exact(additional)
    }
    pub fn values_shrink_to(&mut self, min_capacity: usize) {
        self.values.shrink_to(min_capacity)
    }
    pub fn values_shrink_to_fit(&mut self) {
        self.values.shrink_to_fit()
    }
    pub fn get(&self) -> &T {
        &self.present.data
    }
    pub fn unlogged_get_mut(&mut self) -> &mut T {
        &mut self.present.data
    }
    fn past_end_rare(&self) -> Option<&RareData<T>> {
        self.values.front()
    }
    /// Most past value or `None` if the oldest value is considered to be the present value
    pub fn past_end(&self) -> Option<&T> {
        if self.index == 0 {
            return None;
        }
        self.past_end_rare().map(|rare| &rare.data)
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            return None;
        }
        self.values.pop_front().map(|rare| {
            self.index -= 1;
            self.len -= rare.len();
            rare.data
        })
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.values.drain(self.index..).map(|rare| rare.data)
    }
    pub fn clear(&mut self, present: T) {
        self.values.clear();
        self.present = RareData {
            data: present,
            skips: Packed(0),
        };
        self.index = 0;
        self.len = 0;
        self.skips = 0;
    }
    pub fn push_present(&mut self, value: Option<T>) {
        self.values.truncate(self.index);
        match value {
            None => {
                self.skips += 1;
                self.present.skips = Packed(self.skips);
            }
            Some(value) => {
                self.present.skips = Packed(self.skips);
                let previous = core::mem::replace(
                    &mut self.present,
                    RareData {
                        data: value,
                        skips: Packed(0),
                    },
                );
                self.values.push_back(previous);
                self.skips = 0;
                self.index += 1;
            }
        }
        self.len += 1;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.len -= 1;
            return Ok(());
        }
        self.index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        let entry = self.values.get_mut(self.index).expect(BACKWARD_EXPECT_MSG);
        core::mem::swap(&mut self.present, entry);
        self.skips = self.present.skips.0;
        self.len -= 1;
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        if self.skips < self.present.skips.0 {
            self.len += 1;
            self.skips += 1;
            Ok(())
        } else if let Some(entry) = self.values.get_mut(self.index) {
            self.len += 1;
            self.index += 1;
            self.skips = 0;
            core::mem::swap(&mut self.present, entry);
            Ok(())
        } else {
            Err(OutOfLog)
        }
    }
    pub fn pop_past_by_len(&mut self, meta: &RevMeta, pushes_per_frame: usize) -> Option<T> {
        let excessive_len = self.len.checked_sub(meta.past_len() * pushes_per_frame)?;
        let past_end = self.past_end_rare()?;
        if excessive_len >= past_end.len() {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(
        &mut self,
        meta: &RevMeta,
        pushes_per_frame: usize,
    ) -> impl LogIter<T> {
        let past_len = (meta.now() - meta.range().start) * pushes_per_frame;
        let mut drain_amount = 0;
        for entry in self.values.iter() {
            let less = self.len - entry.len();
            match less.cmp(&past_len) {
                Ordering::Greater => {
                    self.len = less;
                    drain_amount += 1;
                }
                Ordering::Less => break,
                Ordering::Equal => {
                    self.len = less;
                    drain_amount += 1;
                    break; // len of entries is never 0, so no further iterations are needed
                }
            }
        }
        self.index -= drain_amount;
        self.values.drain(..drain_amount).map(|rare| rare.data)
    }
}

impl<T: Debug> RareValueLog<WithTimestamp<T>> {
    pub fn pop_past_by_timestamp(&mut self, meta: &RevMeta) -> Option<WithTimestamp<T>> {
        if self.past_end_rare().map_or(false, |entry| {
            entry.data.logged_at.0 + entry.skips.0 < meta.range().start
        }) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_timestamp(&mut self, meta: &RevMeta) -> impl LogIter<WithTimestamp<T>> {
        let partition_point = self
            .values
            .partition_point(|entry| entry.data.logged_at.0 + entry.skips.0 < meta.range().start);
        self.len -= partition_point // sum of to-be-drained values, because of this mapping RareData::len below is not needed, only skips_before_value
            + self
                .values
                .range(..partition_point)
                .map(|entry| entry.skips.0)
                .sum::<usize>();
        self.index -= partition_point;
        self.values.drain(..partition_point).map(|rare| rare.data)
    }
}

impl<T, Amount: Copy> RareValueLog<WithAmount<WithTimestamp<T>, Amount>> {
    pub(crate) fn pop_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<WithAmount<WithTimestamp<T>, Amount>> {
        if self.past_end_rare().map_or(false, |entry| {
            entry.data.entry.logged_at.0 + entry.skips.0 < meta.range().start
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
        let partition_point = self.values.partition_point(|entry| {
            entry.data.entry.logged_at.0 + entry.skips.0 < meta.range().start
        });
        self.len -= partition_point
            + self
                .values
                .range(..partition_point)
                .map(|entry| entry.skips.0)
                .sum::<usize>();
        self.index -= partition_point;
        self.values.drain(..partition_point).map(|rare| rare.data)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use super::*;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [RareValueLog<WithTimestamp<usize>>; 2],
        one_per_frame: [RareValueLog<usize>; 2],
    }

    impl MetaAndLogs {
        fn new(present: usize, max_len: Option<NonZeroUsize>) -> Self {
            let meta = RevMeta::new(max_len, 0, false);
            let with_timestamp =
                RareValueLog::<WithTimestamp<usize>>::from(meta.with_timestamp(present));
            let one_per_frame = RareValueLog::from(present);
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: [with_timestamp.clone(), with_timestamp],
                one_per_frame: [one_per_frame.clone(), one_per_frame],
            }
        }
        fn forward(
            &mut self,
            value: usize,
            push: bool,
            minimum_log_len: usize,
            expected_values_len: usize,
        ) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update_inner();

            let with_timestamp = self.meta.with_timestamp(value);

            self.with_timestamp[0].push_present(push.then_some(with_timestamp));
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].pop_past_by_timestamp(&self.meta);
            assert!(
                self.with_timestamp[0].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );
            assert_eq!(
                self.with_timestamp[0].values_len(),
                expected_values_len,
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

            self.with_timestamp[1].push_present(push.then_some(with_timestamp));
            let middle = self.with_timestamp[1].clone();
            let _ = self.with_timestamp[1].drain_past_by_timestamp(&self.meta);
            assert!(
                self.with_timestamp[1].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );
            assert_eq!(
                self.with_timestamp[1].values_len(),
                expected_values_len,
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

            self.one_per_frame[0].push_present(push.then_some(value.into()));
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(&self.meta, 1);
            assert!(
                self.one_per_frame[0].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );
            assert_eq!(
                self.one_per_frame[0].values_len(),
                expected_values_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );
            assert_eq!(
                *self.one_per_frame[0].get(),
                value,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );

            self.one_per_frame[1].push_present(push.then_some(value.into()));
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(&self.meta, 1);
            assert!(
                self.one_per_frame[1].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
            assert_eq!(
                self.one_per_frame[1].values_len(),
                expected_values_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
            assert_eq!(
                *self.one_per_frame[1].get(),
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
                        "\npreviously: {previous:#?}\nnow: {self:#?}"
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
                    let value = *self.one_per_frame[0].get();
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
                    let value = *self.one_per_frame[1].get();
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
                    let value = *self.one_per_frame[0].get();
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
                    let value = *self.one_per_frame[1].get();
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

        // minimum_log_len remains < max_len because the current value is not considered to be part of the log
        meta_and_logs.forward(0, false, 0, 0);
        meta_and_logs.forward(0, false, 1, 0);
        meta_and_logs.forward(0, false, 2, 0);
        meta_and_logs.forward(0, false, 2, 0);

        // values_len is reduced by max_len
        meta_and_logs.forward(1, true, 2, 1);
        meta_and_logs.forward(1, false, 2, 1);
        meta_and_logs.forward(1, false, 2, 0);

        meta_and_logs.forward(2, true, 2, 1);
        meta_and_logs.forward(2, false, 2, 1);

        meta_and_logs.backward_log(Ok(2));
        meta_and_logs.backward_log(Ok(1));
        //meta_and_logs.backward_log(Err(OutOfLog)); // todo:
        // - panics because T from before log start is still in the log because the entry's skips is needed
        // - the log cannot determine the log end
        // - this is not an issue for usage as pop_front_by_... does not guarantee a minimal log len, just minimal values len

        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(2));
        meta_and_logs.backward_log(Ok(1));
        meta_and_logs.forward(1, false, 2, 0);
    }
}
