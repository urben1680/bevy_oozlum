use core::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::{TryReserveError, VecDeque},
};

use crate::meta::RevMeta;

use super::{LogIter, OutOfLog, Packed, RareData, WithAmount, WithTimestamp, BACKWARD_EXPECT_MSG};

#[derive(Debug, Clone)]
pub struct RareTransitionLog<T> {
    /// RareData.skips represents the number of None pushes before the transition in the struct
    transitions: VecDeque<RareData<T>>,
    index: usize,
    skips: usize,
    /// Used to check for OutOfLog error when calling `self.forward_log`
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
    pub fn with_capacity(transitions_capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(transitions_capacity),
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
    fn past_end_rare(&self) -> Option<&RareData<T>> {
        self.transitions.front()
    }
    pub fn past_end(&self) -> Option<&T> {
        self.past_end_rare().map(|rare| &rare.data)
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            // if the current log position is at the past end, transitions.front() is not a past value but a future value
            return None;
        }
        self.transitions.pop_front().map(|rare| {
            self.index -= 1;
            self.len -= rare.len();
            rare.data
        })
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.skips_max = self.skips;
        self.transitions.drain(self.index..).map(|rare| rare.data)
    }
    pub fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
        self.len = 0;
        self.skips = 0;
        self.skips_max = 0;
    }
    pub fn push_present(&mut self, transition: Option<T>) {
        self.transitions.truncate(self.index);
        match transition {
            None => {
                self.skips += 1;
            }
            Some(transition) => {
                self.transitions.push_back(RareData {
                    data: transition,
                    skips: Packed(self.skips),
                });
                self.skips = 0;
                self.index += 1;
            }
        }
        self.skips_max = self.skips;
        self.len += 1;
    }
    pub fn backward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.len -= 1;
            Ok(None)
        } else {
            self.index = self.index.checked_sub(1).ok_or(OutOfLog)?;
            let entry = self
                .transitions
                .get_mut(self.index)
                .expect(BACKWARD_EXPECT_MSG);
            self.skips = entry.skips.0;
            self.len -= 1;
            Ok(Some(&mut entry.data))
        }
    }
    pub fn forward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if let Some(entry) = self.transitions.get_mut(self.index) {
            self.len += 1;
            if self.skips < entry.skips.0 {
                self.skips += 1;
                Ok(None)
            } else {
                self.index += 1;
                self.skips = 0;
                Ok(Some(&mut entry.data))
            }
        } else if self.skips < self.skips_max {
            self.len += 1;
            self.skips += 1;
            Ok(None)
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
        for entry in self.transitions.iter() {
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
        self.transitions.drain(..drain_amount).map(|rare| rare.data)
    }
}

impl<T: Debug> RareTransitionLog<WithTimestamp<T>> {
    pub fn pop_past_by_timestamp(&mut self, meta: &RevMeta) -> Option<WithTimestamp<T>> {
        if self.past_end().map_or(false, |entry| {
            // include range().start because this entry instructs how to transition from range().start to range().start - 1
            entry.logged_at.0 <= meta.range().start
        }) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_timestamp(&mut self, meta: &RevMeta) -> impl LogIter<WithTimestamp<T>> {
        let partition_point = self
            .transitions
            .partition_point(|entry| entry.data.logged_at.0 <= meta.range().start);
        self.len -= partition_point // sum of to-be-drained transitions, because of this mapping RareData::len below is not needed, only skips_before_value
            + self
                .transitions
                .range(..partition_point)
                .map(|entry| entry.skips.0)
                .sum::<usize>();
        self.index -= partition_point;
        self.transitions
            .drain(..partition_point)
            .map(|rare| rare.data)
    }
}

impl<T, Amount: Copy> RareTransitionLog<WithAmount<WithTimestamp<T>, Amount>> {
    pub(crate) fn pop_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<WithAmount<WithTimestamp<T>, Amount>> {
        if self.past_end().map_or(false, |entry| {
            // include range().start because this entry instructs how to transition from range().start to range().start - 1
            entry.entry.logged_at.0 <= meta.range().start
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
            .partition_point(|entry| entry.data.entry.logged_at.0 <= meta.range().start);
        self.len -= partition_point
            + self
                .transitions
                .range(..partition_point)
                .map(|entry| entry.skips.0)
                .sum::<usize>();
        self.index -= partition_point;
        self.transitions
            .drain(..partition_point)
            .map(|rare| rare.data)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use super::*;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [RareTransitionLog<WithTimestamp<usize>>; 2],
        one_per_frame: [RareTransitionLog<usize>; 2],
    }

    impl MetaAndLogs {
        fn new(max_len: Option<NonZeroUsize>) -> Self {
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: Default::default(),
                one_per_frame: Default::default(),
            }
        }
        fn forward(
            &mut self,
            transition: Option<usize>,
            minimum_log_len: usize,
            expected_transitions_len: usize,
        ) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update();

            let with_timestamp = transition.map(|transition| self.meta.with_timestamp(transition));

            self.with_timestamp[0].pop_past_by_timestamp(&self.meta);
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].push_present(with_timestamp);
            assert!(
                self.with_timestamp[0].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );
            assert_eq!(
                self.with_timestamp[0].transitions_len(),
                expected_transitions_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );

            let _ = self.with_timestamp[1].drain_past_by_timestamp(&self.meta);
            let middle = self.with_timestamp[1].clone();
            self.with_timestamp[1].push_present(with_timestamp);
            assert!(
                self.with_timestamp[1].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );
            assert_eq!(
                self.with_timestamp[1].transitions_len(),
                expected_transitions_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );

            self.one_per_frame[0].push_present(transition.map(Into::into));
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
                self.one_per_frame[0].transitions_len(),
                expected_transitions_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );

            self.one_per_frame[1].push_present(transition.map(Into::into));
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
                self.one_per_frame[1].transitions_len(),
                expected_transitions_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
        }
        fn backward_log(&mut self, expected_transition: Result<Option<usize>, OutOfLog>) {
            let previous = self.clone();
            match expected_transition {
                Ok(_) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    let transition = self.with_timestamp[0]
                        .backward_log()
                        .map(|ok| ok.map(|with_timestamp| with_timestamp.data));
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let transition = self.with_timestamp[1]
                        .backward_log()
                        .map(|ok| ok.map(|with_timestamp| with_timestamp.data));
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let transition = self.one_per_frame[0].backward_log().map(|ok| ok.cloned());
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let transition = self.one_per_frame[1].backward_log().map(|ok| ok.cloned());
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
        fn forward_log(&mut self, expected_transition: Result<Option<usize>, OutOfLog>) {
            let previous = self.clone();
            match expected_transition {
                Ok(_) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    let transition = self.with_timestamp[0]
                        .forward_log()
                        .map(|with_timestamp| with_timestamp.map(|transition| transition.data));
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let transition = self.with_timestamp[1]
                        .forward_log()
                        .map(|with_timestamp| with_timestamp.map(|transition| transition.data));
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let transition = self.one_per_frame[0]
                        .forward_log()
                        .map(|data| data.cloned());
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let transition = self.one_per_frame[1]
                        .forward_log()
                        .map(|data| data.cloned());
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

        meta_and_logs.forward(None, 1, 0);
        meta_and_logs.forward(Some(1), 2, 1);
        // pop_front called internally
        meta_and_logs.forward(Some(2), 2, 2);
        meta_and_logs.forward(None, 2, 1);

        meta_and_logs.backward_log(Ok(None));
        meta_and_logs.backward_log(Ok(Some(2)));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err(OutOfLog));

        meta_and_logs.forward_log(Ok(Some(2)));
        meta_and_logs.forward_log(Ok(None));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        // todo: would this test fail if no value was pushed at the second forward? same situation as RareValueLog
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(None));
        meta_and_logs.backward_log(Ok(Some(2)));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(Some(3), 1, 1);
    }
}
