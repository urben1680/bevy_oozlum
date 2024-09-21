use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::{reflect::Reflect, utils::tracing::error};

use super::{BorrowTimestamp, LogIter, OutOfLog, PackedTime, RareValue, INDEX_OOB};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareStateLog<T> {
    /// RareValue.skips represents the number of None pushes after the state in the struct
    states: VecDeque<RareValue<T>>,
    present: RareValue<T>,
    index: usize,
    skips: usize,
    len: usize,
}

#[derive(Debug)]
#[allow(dead_code)]
struct RareStateLogDebug {
    states_len: usize,
    present_skips: usize,
    index: usize,
    skips: usize,
    len: usize,
}

impl<T> From<T> for RareStateLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> RareStateLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present: RareValue {
                value: present,
                skips: PackedTime::MIN,
            },
            index: 0,
            skips: 0,
            len: 0,
        }
    }

    pub fn with_capacity(present: T, states_capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(states_capacity),
            present: RareValue {
                value: present,
                skips: PackedTime::MIN,
            },
            index: 0,
            skips: 0,
            len: 0,
        }
    }
    pub fn into_inner(self) -> T {
        self.present.value
    }
    pub fn log_len(&self) -> usize {
        self.len
    }
    pub fn states_len(&self) -> usize {
        self.states.len()
    }
    pub fn states_capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn states_is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn states_reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn states_shrink_to_fit(&mut self) {
        self.states.shrink_to_fit()
    }
    pub fn get(&self) -> &T {
        &self.present.value
    }
    pub fn unlogged_get_mut(&mut self) -> &mut T {
        &mut self.present.value
    }
    fn past_end_rare(&self) -> Option<&RareValue<T>> {
        self.states.front()
    }
    /// Most past state or `None` if the oldest state is considered to be the present state
    pub fn past_end(&self) -> Option<&T> {
        if self.index == 0 {
            return None;
        }
        self.past_end_rare().map(|rare| &rare.value)
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            return None;
        }
        self.states.pop_front().map(|rare| {
            self.index -= 1;
            self.len -= rare.len();
            rare.value
        })
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.states.drain(self.index..).map(|rare| rare.value)
    }
    pub fn clear(&mut self) {
        self.states.clear();
        self.present.skips = PackedTime::MIN;
        self.index = 0;
        self.len = 0;
        self.skips = 0;
    }
    pub fn clear_with(&mut self, present: T) {
        self.states.clear();
        self.present = RareValue {
            value: present,
            skips: PackedTime::MIN,
        };
        self.index = 0;
        self.len = 0;
        self.skips = 0;
    }
    pub fn push_present(&mut self, state: Option<T>) {
        self.states.truncate(self.index);
        self.len += 1;
        match state {
            None => {
                self.skips += 1;
                self.present.skips = PackedTime::from_internal(self.skips);
            }
            Some(state) => {
                self.present.skips = PackedTime::from_internal(self.skips);
                let previous = core::mem::replace(
                    &mut self.present,
                    RareValue {
                        value: state,
                        skips: PackedTime::MIN,
                    },
                );
                self.states.push_back(previous);
                self.skips = 0;
                self.index += 1;
            }
        }
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.len -= 1;
            return Ok(());
        }
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        if let Some(entry) = self.states.get_mut(index) {
            self.index = index;
            core::mem::swap(&mut self.present, entry);
            self.skips = self.present.skips.into();
            self.len -= 1;
            return Ok(());
        }

        let debug_struct = RareStateLogDebug {
            states_len: self.states.len(),
            present_skips: self.present.skips.into(),
            index: self.index,
            skips: self.skips,
            len: self.len,
        };

        error!("{INDEX_OOB}, {debug_struct:#?}");
        Err(OutOfLog)
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        if self.skips < self.present.skips.into() {
            self.len += 1;
            self.skips += 1;
            Ok(())
        } else if let Some(entry) = self.states.get_mut(self.index) {
            self.len += 1;
            self.index += 1;
            self.skips = 0;
            core::mem::swap(&mut self.present, entry);
            Ok(())
        } else {
            Err(OutOfLog)
        }
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        let excessive_len = self.len.checked_sub(max_past_len)?;
        let past_end = self.past_end_rare()?;
        if excessive_len >= past_end.len() {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
        let mut drain_amount = 0;
        for entry in self.states.iter() {
            let less = self.len - entry.len();
            if less < max_past_len {
                break;
            }
            self.len = less;
            drain_amount += 1;
        }
        self.index -= drain_amount;
        self.states.drain(..drain_amount).map(|rare| rare.value)
    }
}

impl<B: BorrowTimestamp> RareStateLog<B> {
    pub fn pop_past_by_timestamp(&mut self, log_start: usize) -> Option<B> {
        let entry = self.past_end_rare()?;
        let logged_at = entry.value.borrow_timestamp().logged_at();
        let skips = entry.skips();
        if logged_at + skips < log_start {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_timestamp(&mut self, log_start: usize) -> impl LogIter<B> {
        let partition_point = self.states.partition_point(|entry| {
            let logged_at = entry.value.borrow_timestamp().logged_at();
            let skips = entry.skips();
            logged_at + skips < log_start
        });
        self.len -= partition_point // sum of to-be-drained states, because of this mapping RareValue::len below is not needed, only skips_before_state
            + self
                .states
                .range(..partition_point)
                .map(RareValue::skips)
                .sum::<usize>();
        self.index -= partition_point;
        self.states.drain(..partition_point).map(|rare| rare.value)
    }
    pub fn reduce_timestamps(&mut self, by: usize) -> impl LogIter<B> {
        let reduced_at = self
            .states
            .range_mut(..self.index)
            .position(|with_timestamp| {
                with_timestamp
                    .value
                    .borrow_timestamp()
                    .logged_at()
                    .checked_sub(by)
                    .inspect(|reduced| {
                        with_timestamp.value.borrow_timestamp_mut().logged_at =
                            PackedTime::from_internal(*reduced)
                    })
                    .is_some()
            });
        self.present.value.borrow_timestamp_mut().logged_at = match reduced_at {
            Some(_) => {
                PackedTime::from_internal(self.present.value.borrow_timestamp().logged_at() - by)
            }
            None => {
                let logged_at = self.present.value.borrow_timestamp().logged_at();
                match logged_at.checked_sub(by) {
                    Some(reduced) => PackedTime::from_internal(reduced),
                    None => panic!(
                        "present state was logged at {logged_at} which cannot be reduced by {by}"
                    ),
                }
            }
        };
        let reduced_at = reduced_at.unwrap_or(self.index);
        for with_timestamp in self.states.range_mut(reduced_at..) {
            let logged_at = with_timestamp.value.borrow_timestamp().logged_at();
            with_timestamp.value.borrow_timestamp_mut().logged_at =
                PackedTime::from_internal(logged_at - by);
        }
        self.index -= reduced_at;
        self.states.drain(..reduced_at).map(|rare| rare.value)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use super::*;

    use crate::{log::WithTimestamp, meta::RevMeta};

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [RareStateLog<WithTimestamp<usize>>; 2],
        one_per_frame: [RareStateLog<usize>; 2],
    }

    impl MetaAndLogs {
        fn new(present: usize, max_len: Option<NonZeroUsize>) -> Self {
            let meta = RevMeta::new(max_len, 0, false);
            let with_timestamp =
                RareStateLog::<WithTimestamp<usize>>::from(meta.with_timestamp(present));
            let one_per_frame = RareStateLog::from(present);
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: [with_timestamp.clone(), with_timestamp],
                one_per_frame: [one_per_frame.clone(), one_per_frame],
            }
        }
        fn forward(
            &mut self,
            state: usize,
            push: bool,
            minimum_log_len: usize,
            expected_states_len: usize,
        ) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update();

            let with_timestamp = self.meta.with_timestamp(state);

            self.with_timestamp[0].push_present(push.then_some(with_timestamp));
            let middle = self.with_timestamp[0].clone();
            self.with_timestamp[0].pop_past_by_timestamp(self.meta.log_range().start);
            assert!(
                self.with_timestamp[0].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );
            assert_eq!(
                self.with_timestamp[0].states_len(),
                expected_states_len,
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

            self.with_timestamp[1].push_present(push.then_some(with_timestamp));
            let middle = self.with_timestamp[1].clone();
            let _ = self.with_timestamp[1].drain_past_by_timestamp(self.meta.log_range().start);
            assert!(
                self.with_timestamp[1].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );
            assert_eq!(
                self.with_timestamp[1].states_len(),
                expected_states_len,
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

            self.one_per_frame[0].push_present(push.then_some(state.into()));
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(self.meta.past_len());
            assert!(
                self.one_per_frame[0].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );
            assert_eq!(
                self.one_per_frame[0].states_len(),
                expected_states_len,
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

            self.one_per_frame[1].push_present(push.then_some(state.into()));
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(self.meta.past_len());
            assert!(
                self.one_per_frame[1].log_len() >= minimum_log_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
            assert_eq!(
                self.one_per_frame[1].states_len(),
                expected_states_len,
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
                        "\npreviously: {previous:#?}\nnow: {self:#?}"
                    );
                    self.meta.update();

                    let is_ok = self.with_timestamp[0].backward_log().is_ok();
                    let state = self.with_timestamp[0].get().value;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );
                    assert_eq!(
                        state, expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let is_ok = self.with_timestamp[1].backward_log().is_ok();
                    let state = self.with_timestamp[1].get().value;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );
                    assert_eq!(
                        state, expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let is_ok = self.one_per_frame[0].backward_log().is_ok();
                    let state = *self.one_per_frame[0].get();
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );
                    assert_eq!(
                        state, expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let is_ok = self.one_per_frame[1].backward_log().is_ok();
                    let state = *self.one_per_frame[1].get();
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                    assert_eq!(
                        state, expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                }
                Err(OutOfLog) => {
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
        fn forward_log(&mut self, expected_state: Result<usize, OutOfLog>) {
            let previous = self.clone();
            match expected_state {
                Ok(expected_state) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    let is_ok = self.with_timestamp[0].forward_log().is_ok();
                    let state = self.with_timestamp[0].get().value;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );
                    assert_eq!(
                        state, expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let is_ok = self.with_timestamp[1].forward_log().is_ok();
                    let state = self.with_timestamp[1].get().value;
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );
                    assert_eq!(
                        state, expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let is_ok = self.one_per_frame[0].forward_log().is_ok();
                    let state = *self.one_per_frame[0].get();
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );
                    assert_eq!(
                        state, expected_state,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let is_ok = self.one_per_frame[1].forward_log().is_ok();
                    let state = *self.one_per_frame[1].get();
                    assert!(
                        is_ok,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[1], self.one_per_frame[1]
                    );
                    assert_eq!(
                        state, expected_state,
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

        // minimum_log_len remains < max_len because the current state is not considered to be part of the log
        meta_and_logs.forward(0, false, 0, 0);
        meta_and_logs.forward(0, false, 1, 0);
        meta_and_logs.forward(0, false, 2, 0);
        meta_and_logs.forward(0, false, 2, 0);

        // states_len is reduced by max_len
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
        // - this is not an issue for usage as pop_front_by_... does not guarantee a minimal log len, just minimal states len

        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(2));
        meta_and_logs.backward_log(Ok(1));
        meta_and_logs.forward(1, false, 2, 0);
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareStateLog<WithTimestamp<usize>>>();
    }
}
