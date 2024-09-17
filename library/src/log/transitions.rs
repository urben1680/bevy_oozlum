use std::{
    collections::{TryReserveError, VecDeque},
    fmt::Debug,
};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

use super::{
    AmountErr, LogIter, LogMut, OutOfLog, PackedUSize, TransitionLog, ValueEntry, WithAmount,
    WithTimestamp,
};

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionsLog<T, U = (), Amount = PackedUSize>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    amounts: TransitionLog<WithAmount<U, Amount>>,
    transitions: VecDeque<T>,
    index: usize,
}

impl<T, U, Amount> Default for TransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U, Amount> TransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    pub const fn new() -> Self {
        Self {
            amounts: TransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacities(log_capacity: usize, transitions_capacity: usize) -> Self {
        Self {
            amounts: TransitionLog::with_capacity(log_capacity),
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
        }
    }
    pub fn log_len(&self) -> usize {
        self.amounts.len()
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn log_capacity(&self) -> usize {
        self.amounts.capacity()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn log_is_empty(&self) -> bool {
        self.amounts.is_empty()
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn log_reserve(&mut self, additional: usize) {
        self.amounts.reserve(additional)
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn log_reserve_exact(&mut self, additional: usize) {
        self.amounts.reserve_exact(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn log_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn log_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.amounts.try_reserve_exact(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn log_shrink_to(&mut self, min_capacity: usize) {
        self.amounts.shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn log_shrink_to_fit(&mut self) {
        self.amounts.shrink_to_fit()
    }
    pub fn transitions_shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }
    pub fn past_end(&self) -> Option<&U> {
        self.amounts
            .past_end()
            .map(|with_amount| &with_amount.entry)
    }
    pub fn pop_past(&mut self) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn try_push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<impl LogIter<T>, U, Amount>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let new_amount = self.transitions.len() - self.index;
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.transitions.len();
                self.amounts.push_present(WithAmount { entry, amount });
                Ok(())
            }
            Err(err) => Err(AmountErr {
                entry,
                values: self.transitions.drain(self.index..),
                err,
            }),
        }
    }
    pub fn push_present<Out: Into<U>>(&mut self, c: impl FnOnce(LogMut<T>) -> Out) {
        self.try_push_present(c)
            .unwrap_or_else(AmountErr::warn::<Self, _>)
    }
    pub fn drain_future(&mut self) -> (impl LogIter<T>, impl LogIter<U>) {
        (
            self.transitions.drain(self.index..),
            self.amounts
                .drain_future()
                .map(|with_amount| with_amount.entry),
        )
    }
    pub fn clear(&mut self) {
        self.transitions.clear();
        self.amounts.clear();
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<ValueEntry<impl LogIter<&mut T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let with_amount = self.amounts.backward_log()?;
        self.index -= with_amount.amount();
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut with_amount.entry,
        })
    }
    pub fn forward_log(&mut self) -> Result<ValueEntry<impl LogIter<&mut T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let with_amount = self.amounts.forward_log()?;
        self.index += with_amount.amount();
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut with_amount.entry,
        })
    }
    pub fn pop_past_by_len(
        &mut self,
        max_past_len: usize,
    ) -> Option<ValueEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past_by_len(max_past_len)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_len(max_past_len)
            .map(|with_amount| with_amount.amount())
            .sum();
        self.index -= amount;
        self.transitions.drain(..amount)
    }
    fn drain_past_by_amount(
        &mut self,
        with_amount: WithAmount<U, Amount>,
    ) -> ValueEntry<impl LogIter<T>, U> {
        let amount = with_amount.amount();
        self.index -= amount;
        ValueEntry {
            value: self.transitions.drain(..amount),
            entry: with_amount.entry,
        }
    }
}

impl<T, U, Amount> TransitionsLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    pub fn pop_past_by_timestamp(
        &mut self,
        log_start: usize,
    ) -> Option<ValueEntry<impl LogIter<T>, WithTimestamp<U>>> {
        self.amounts
            .pop_past_by_timestamp(log_start)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_timestamp(&mut self, log_start: usize) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_timestamp(log_start)
            .map(|with_amount| with_amount.amount())
            .sum();
        self.index -= amount;
        self.transitions.drain(..amount)
    }
}

#[cfg(test)]
mod test {
    use std::{borrow::Borrow, num::NonZeroUsize};

    use super::*;

    use crate::meta::RevMeta;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [TransitionsLog<usize, WithTimestamp, u8>; 2],
        one_per_frame: [TransitionsLog<usize, (), u8>; 2],
    }

    fn collect(iter: impl IntoIterator<Item: Borrow<usize>>) -> Vec<usize> {
        iter.into_iter().map(|val| val.borrow().clone()).collect()
    }

    impl MetaAndLogs {
        fn new(max_len: Option<NonZeroUsize>) -> Self {
            Self {
                meta: RevMeta::new(max_len, 0, false),
                with_timestamp: Default::default(),
                one_per_frame: Default::default(),
            }
        }
        fn forward<const N: usize>(
            &mut self,
            transitions: Result<[usize; N], [usize; N]>,
            expected_len: usize,
        ) {
            let previous = self.clone();

            self.meta.queue_forward();
            self.meta.update();

            let (transitions, expected_ok) = match transitions {
                Ok(transitions) => (transitions, true),
                Err(transitions) => (transitions, false),
            };

            self.with_timestamp[0].pop_past_by_timestamp(self.meta.log_range().start);
            let middle = self.with_timestamp[0].clone();
            let is_ok = self.with_timestamp[0]
                .try_push_present(|mut log| {
                    log.extend(transitions);
                    self.meta.with_timestamp(())
                })
                .is_ok();
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.with_timestamp[0], self.with_timestamp[0]
            );
            assert_eq!(
                self.with_timestamp[0].log_len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[0],
                self.with_timestamp[0]
            );

            let _ = self.with_timestamp[1].drain_past_by_timestamp(self.meta.log_range().start);
            let middle = self.with_timestamp[1].clone();
            let is_ok = self.with_timestamp[1]
                .try_push_present(|mut log| {
                    log.extend(transitions);
                    self.meta.with_timestamp(())
                })
                .is_ok();
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.with_timestamp[1], self.with_timestamp[1]
            );
            assert_eq!(
                self.with_timestamp[1].log_len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );

            let is_ok = self.one_per_frame[0]
                .try_push_present(|mut log| {
                    log.extend(transitions);
                })
                .is_ok();
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(self.meta.past_len());
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.one_per_frame[0], self.one_per_frame[0]
            );
            assert_eq!(
                self.one_per_frame[0].log_len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[0],
                self.one_per_frame[0]
            );

            let is_ok = self.one_per_frame[1]
                .try_push_present(|mut log| {
                    log.extend(transitions);
                })
                .is_ok();
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(self.meta.past_len());
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.one_per_frame[1], self.one_per_frame[1]
            );
            assert_eq!(
                self.one_per_frame[1].log_len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.one_per_frame[1],
                self.one_per_frame[1]
            );
        }
        fn backward_log<const N: usize>(
            &mut self,
            expected_transitions: Result<[usize; N], [OutOfLog; N]>,
        ) {
            let previous = self.clone();
            match expected_transitions {
                Ok(expected_transitions) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    let transitions = self.with_timestamp[0]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    let transitions = self.with_timestamp[1]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    let transitions = self.one_per_frame[0]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    let transitions = self.one_per_frame[1]
                        .backward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(_) => {
                    assert_eq!(
                        self.with_timestamp[0].backward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].backward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].backward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].backward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
            }
        }
        fn forward_log<const N: usize>(
            &mut self,
            expected_transitions: Result<[usize; N], [OutOfLog; N]>,
        ) {
            let previous = self.clone();
            match expected_transitions {
                Ok(expected_transitions) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update();

                    let transitions = self.with_timestamp[0]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    let transitions = self.with_timestamp[1]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    let transitions = self.one_per_frame[0]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    let transitions = self.one_per_frame[1]
                        .forward_log()
                        .map(|entry| collect(entry.value));
                    assert_eq!(
                        transitions,
                        Ok(collect(expected_transitions)),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
                    );
                }
                Err(_) => {
                    assert_eq!(
                        self.with_timestamp[0].forward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );
                    assert_eq!(
                        self.with_timestamp[1].forward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );
                    assert_eq!(
                        self.one_per_frame[0].forward_log().err(),
                        Some(OutOfLog),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );
                    assert_eq!(
                        self.one_per_frame[1].forward_log().err(),
                        Some(OutOfLog),
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

        meta_and_logs.forward(Ok([1; 1]), 1);
        meta_and_logs.forward(Ok([2; 2]), 2);
        // pop_front called internally
        meta_and_logs.forward(Ok([3; 3]), 2);

        meta_and_logs.backward_log(Ok([3; 3]));
        meta_and_logs.backward_log(Ok([2; 2]));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err([OutOfLog]));

        meta_and_logs.forward_log(Ok([2; 2]));
        meta_and_logs.forward_log(Ok([3; 3]));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log(Err([OutOfLog]));

        meta_and_logs.backward_log(Ok([3; 3]));
        meta_and_logs.backward_log(Ok([2; 2]));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(Ok([4; 4]), 1);

        // amount of transitions is stored as u8, cannot store more than 255 transitions per push
        meta_and_logs.forward(Err([256; 256]), 1);
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty()
            .register::<TransitionsLog<usize, WithTimestamp<u8>, u8>>();
    }
}
