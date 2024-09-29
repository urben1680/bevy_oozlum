use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::{
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::error,
};

use super::{LogIter, LoggedAt, OutOfLog, PackedTime, RareValue, INDEX_OOB};

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareTransitionLog<T> {
    /// RareValue.skips represents the number of None pushes before the transition in the struct
    transitions: VecDeque<RareValue<T>>,
    index: usize,
    skips: usize,
    /// Used to check for OutOfLog error when calling `self.forward_log`
    skips_max: usize,
    len: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::{RareTransitionLog, RareValue};

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for RareTransitionLog<T> {
        type Se<'se> = (
            WithCapacityWrapper<&'se VecDeque<RareValue<T>>>,
            usize,
            usize,
            usize,
            usize,
        );
        type De = (
            WithCapacityWrapper<VecDeque<RareValue<T>>>,
            usize,
            usize,
            usize,
            usize,
        );
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                WithCapacityWrapper(&self.transitions),
                self.index,
                self.skips,
                self.skips_max,
                self.len,
            )
        }
        fn from_with_capacity(with_capacity: Self::De) -> Self {
            Self {
                transitions: with_capacity.0 .0,
                index: with_capacity.1,
                skips: with_capacity.2,
                skips_max: with_capacity.3,
                len: with_capacity.4,
            }
        }
    }

    impl<T> LoglessWithCapacity for RareTransitionLog<T> {
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
    fn past_end_rare(&self) -> Option<&RareValue<T>> {
        self.transitions.front()
    }
    pub fn past_end(&self) -> Option<&T> {
        self.past_end_rare().map(|rare| &rare.value)
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            // if the current log position is at the past end, transitions.front() is not a past value but a future value
            return None;
        }
        self.transitions.pop_front().map(|rare| {
            self.index -= 1;
            self.len -= rare.len();
            rare.value
        })
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.skips_max = self.skips;
        self.transitions.drain(self.index..).map(|rare| rare.value)
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
                let skips = PackedTime::from_internal(self.skips);
                self.transitions.push_back(RareValue {
                    value: transition,
                    skips,
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
            let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
            let transitions_len = self.transitions.len();
            if let Some(entry) = self.transitions.get_mut(index) {
                self.index = index;
                self.skips = entry.skips.into();
                self.len -= 1;
                return Ok(Some(&mut entry.value));
            }

            #[derive(Debug)]
            #[allow(dead_code)]
            struct RareTransitionLogDebug {
                transitions_len: usize,
                index: usize,
                skips: usize,
                skips_max: usize,
                len: usize,
            }

            let debug_struct = RareTransitionLogDebug {
                transitions_len,
                index: self.index,
                skips: self.skips,
                skips_max: self.skips_max,
                len: self.len,
            };

            error!("{INDEX_OOB}, {debug_struct:#?}");
            Err(OutOfLog)
        }
    }
    pub fn forward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if let Some(entry) = self.transitions.get_mut(self.index) {
            self.len += 1;
            if self.skips < entry.skips.into() {
                self.skips += 1;
                Ok(None)
            } else {
                self.index += 1;
                self.skips = 0;
                Ok(Some(&mut entry.value))
            }
        } else if self.skips < self.skips_max {
            self.len += 1;
            self.skips += 1;
            Ok(None)
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
        for entry in self.transitions.iter() {
            let less = self.len - entry.len();
            if less < max_past_len {
                break;
            }
            self.len = less;
            drain_amount += 1;
        }
        self.index -= drain_amount;
        self.transitions
            .drain(..drain_amount)
            .map(|rare| rare.value)
    }
}

impl<T: LoggedAt> RareTransitionLog<T> {
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
            .partition_point(|entry| entry.value.logged_at() <= log_start);
        self.len -= partition_point // sum of to-be-drained transitions, because of this mapping RareValue::len below is not needed, only skips_before_value
            + self
                .transitions
                .range(..partition_point)
                .map(|entry| usize::from(entry.skips))
                .sum::<usize>();
        self.index -= partition_point;
        self.transitions
            .drain(..partition_point)
            .map(|rare| rare.value)
    }
    pub fn reduce_timestamps(&mut self, by: usize) -> impl LogIter<T> {
        let reduced_at = self
            .transitions
            .range_mut(..self.index)
            .position(|with_timestamp| {
                with_timestamp
                    .value
                    .logged_at()
                    .checked_sub(by)
                    .inspect(|reduced| {
                        with_timestamp
                            .value
                            .set_logged_at(PackedTime::from_internal(*reduced))
                    })
                    .is_some()
            })
            .unwrap_or(self.index);
        let mut iter = self.transitions.range_mut(reduced_at..);
        if reduced_at == self.index {
            if let Some(with_timestamp) = iter.next() {
                let logged_at = with_timestamp.value.logged_at();
                let logged_at = match logged_at.checked_sub(by) {
                    Some(reduced) => PackedTime::from_internal(reduced),
                    None => panic!(
                        "future transition was logged at {logged_at} which cannot be reduced by {by}"
                    ),
                };
                with_timestamp.value.set_logged_at(logged_at);
            }
        }
        for with_timestamp in iter {
            let logged_at = with_timestamp.value.logged_at();
            with_timestamp
                .value
                .set_logged_at(PackedTime::from_internal(logged_at - by));
        }
        self.index -= reduced_at;
        self.transitions.drain(..reduced_at).map(|rare| rare.value)
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
        with_timestamp: [RareTransitionLog<WithLoggedAt<usize>>; 2],
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

            self.with_timestamp[0].pop_past_by_timestamp(self.meta.log_range().start);
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

            let _ = self.with_timestamp[1].drain_past_by_timestamp(self.meta.log_range().start);
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
            self.one_per_frame[0].pop_past_by_len(self.meta.past_len());
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
            let _ = self.one_per_frame[1].drain_past_by_len(self.meta.past_len());
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
                        .map(|ok| ok.map(|with_timestamp| with_timestamp.value));
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let transition = self.with_timestamp[1]
                        .backward_log()
                        .map(|ok| ok.map(|with_timestamp| with_timestamp.value));
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
                        .map(|with_timestamp| with_timestamp.map(|transition| transition.value));
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[0], self.with_timestamp[0]
                    );

                    let transition = self.with_timestamp[1]
                        .forward_log()
                        .map(|with_timestamp| with_timestamp.map(|transition| transition.value));
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.with_timestamp[1], self.with_timestamp[1]
                    );

                    let transition = self.one_per_frame[0]
                        .forward_log()
                        .map(|value| value.cloned());
                    assert_eq!(
                        transition, expected_transition,
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta, previous.one_per_frame[0], self.one_per_frame[0]
                    );

                    let transition = self.one_per_frame[1]
                        .forward_log()
                        .map(|value| value.cloned());
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

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareTransitionLog<WithLoggedAt<usize>>>();
    }
}
