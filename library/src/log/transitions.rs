use std::{
    collections::{TryReserveError, VecDeque},
    fmt::Debug,
};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{
    amount_to_usize, AmountErr, DataEntry, LogIter, LogMut, NPerFrame, OutOfLog, Packed,
    TransitionLog, WithAmount, WithTimestamp,
};

#[derive(Debug, Clone, Component, Resource)]
pub struct TransitionsLog<T, U: Copy = (), Amount = usize>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
{
    amounts: TransitionLog<WithAmount<U, Amount>>,
    transitions: VecDeque<T>,
    index: usize,
}

impl<T, U: Copy, Amount> Default for TransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U: Copy, Amount> TransitionsLog<T, U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
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
    pub fn past_end(&self) -> Option<U> {
        self.amounts.past_end().map(|with_amount| with_amount.entry)
    }
    pub fn pop_past(&mut self) -> Option<DataEntry<impl LogIter<T>, U>> {
        self.amounts
            .pop_past()
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    // todo: extend methode die den letzten amount vergrößert?
    pub fn push_present<Out: Into<U>>(
        &mut self,
        c: impl FnOnce(LogMut<T>) -> Out,
    ) -> Result<(), AmountErr<impl LogIter<T>, U, Amount>> {
        self.transitions.truncate(self.index);
        let entry = c(LogMut(&mut self.transitions)).into();
        let new_amount = self.transitions.len() - self.index;
        match new_amount.try_into() {
            Ok(amount) => {
                self.index = self.transitions.len();
                self.amounts.push_present(WithAmount {
                    entry,
                    amount: Packed(amount),
                });
                Ok(())
            }
            Err(err) => Err(AmountErr {
                entry,
                data: self.transitions.drain(self.index..),
                err,
            }),
        }
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
    pub fn backward_log(&mut self) -> Result<DataEntry<impl LogIter<&mut T>, U>, OutOfLog> {
        let old_index = self.index;
        let with_amount = self.amounts.backward_log()?;
        self.index -= amount_to_usize(with_amount.amount.0);
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(DataEntry {
            data: iter,
            entry: with_amount.entry,
        })
    }
    pub fn forward_log(&mut self) -> Result<DataEntry<impl LogIter<&mut T>, U>, OutOfLog> {
        let old_index = self.index;
        let with_amount = self.amounts.forward_log()?;
        self.index += amount_to_usize(with_amount.amount.0);
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(DataEntry {
            data: iter,
            entry: with_amount.entry,
        })
    }
    fn drain_past_by_amount(
        &mut self,
        with_amount: WithAmount<U, Amount>,
    ) -> DataEntry<impl LogIter<T>, U> {
        let amount = amount_to_usize(with_amount.amount.0);
        self.index -= amount;
        DataEntry {
            data: self.transitions.drain(..amount),
            entry: with_amount.entry,
        }
    }
}

impl<T, U: Copy, Amount> TransitionsLog<T, WithTimestamp<U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
{
    pub fn pop_past_by_timestamp(
        &mut self,
        meta: &RevMeta,
    ) -> Option<DataEntry<impl LogIter<T>, WithTimestamp<U>>> {
        self.amounts
            .pop_past_by_timestamp(meta)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_timestamp(&mut self, meta: &RevMeta) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_timestamp(meta)
            .map(|entry| amount_to_usize(entry.amount.0))
            .sum();
        self.index -= amount;
        self.transitions.drain(..amount)
    }
}

impl<const N: usize, T, U: Copy, Amount> TransitionsLog<T, NPerFrame<N, U>, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy,
{
    pub fn pop_past_by_len(
        &mut self,
        meta: &RevMeta,
    ) -> Option<DataEntry<impl LogIter<T>, NPerFrame<N, U>>> {
        self.amounts
            .pop_past_by_len(meta)
            .map(|with_amount| self.drain_past_by_amount(with_amount))
    }
    pub fn drain_past_by_len(&mut self, meta: &RevMeta) -> impl LogIter<T> {
        let amount: usize = self
            .amounts
            .drain_past_by_len(meta)
            .map(|entry| amount_to_usize(entry.amount.0))
            .sum();
        self.index -= amount;
        self.transitions.drain(..amount)
    }
}

#[cfg(test)]
mod test {
    use std::{borrow::Borrow, num::NonZeroUsize};

    use crate::log::OnePerFrame;

    use super::*;

    #[derive(Clone, Debug)]
    struct MetaAndLogs {
        meta: RevMeta,
        with_timestamp: [TransitionsLog<usize, WithTimestamp, u8>; 2],
        one_per_frame: [TransitionsLog<usize, OnePerFrame, u8>; 2],
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
            self.meta.update_inner();

            let (transitions, expected_ok) = match transitions {
                Ok(transitions) => (transitions, true),
                Err(transitions) => (transitions, false),
            };

            self.with_timestamp[0].pop_past_by_timestamp(&self.meta);
            let middle = self.with_timestamp[0].clone();
            let is_ok = self.with_timestamp[0]
                .push_present(|mut log| {
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

            let _ = self.with_timestamp[1].drain_past_by_timestamp(&self.meta);
            let middle = self.with_timestamp[0].clone();
            let is_ok = self.with_timestamp[1]
                .push_present(|mut log| {
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
                self.with_timestamp[1].log_len(),
                expected_len,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta,
                previous.with_timestamp[1],
                self.with_timestamp[1]
            );

            let is_ok = self.one_per_frame[0]
                .push_present(|mut log| {
                    log.extend(transitions);
                })
                .is_ok();
            let middle = self.one_per_frame[0].clone();
            self.one_per_frame[0].pop_past_by_len(&self.meta);
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.with_timestamp[0], self.with_timestamp[0]
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
                .push_present(|mut log| {
                    log.extend(transitions);
                })
                .is_ok();
            let middle = self.one_per_frame[1].clone();
            let _ = self.one_per_frame[1].drain_past_by_len(&self.meta);
            assert_eq!(
                is_ok, expected_ok,
                "\nmeta: {:#?}\npreviously: {:#?}\nmiddle: {middle:#?}\nnow: {:#?}",
                self.meta, previous.with_timestamp[0], self.with_timestamp[0]
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
            expected_transitions: Result<[usize; N], OutOfLog>,
        ) {
            let previous = self.clone();
            match expected_transitions {
                Ok(expected_transitions) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() - 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update_inner();

                    let transitions = self.with_timestamp[0].backward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    let transitions = self.with_timestamp[1].backward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    let transitions = self.one_per_frame[0].backward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    let transitions = self.one_per_frame[1].backward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
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
        fn forward_log<const N: usize>(
            &mut self,
            expected_transitions: Result<[usize; N], OutOfLog>,
        ) {
            let previous = self.clone();
            match expected_transitions {
                Ok(expected_transitions) => {
                    assert!(
                        self.meta.queue_log(self.meta.now() + 1).is_ok(),
                        "\npreviously: {previous:?}\nnow: {self:?}"
                    );
                    self.meta.update_inner();

                    let transitions = self.with_timestamp[0].forward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[0],
                        self.with_timestamp[0]
                    );

                    let transitions = self.with_timestamp[1].forward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.with_timestamp[1],
                        self.with_timestamp[1]
                    );

                    let transitions = self.one_per_frame[0].forward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[0],
                        self.one_per_frame[0]
                    );

                    let transitions = self.one_per_frame[1].forward_log().unwrap().data;
                    assert_eq!(
                        collect(transitions),
                        collect(expected_transitions),
                        "\nmeta: {:#?}\npreviously: {:#?}\nnow: {:#?}",
                        self.meta,
                        previous.one_per_frame[1],
                        self.one_per_frame[1]
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

        meta_and_logs.forward(Ok([1, 2]), 1);
        meta_and_logs.forward(Ok([3, 4, 5]), 2);
        // pop_front called internally
        meta_and_logs.forward(Ok([6, 7, 8, 9]), 2);

        meta_and_logs.backward_log(Ok([6, 7, 8, 9]));
        meta_and_logs.backward_log(Ok([3, 4, 5]));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log::<0>(Err(OutOfLog));

        meta_and_logs.forward_log(Ok([3, 4, 5]));
        meta_and_logs.forward_log(Ok([6, 7, 8, 9]));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        meta_and_logs.forward_log::<0>(Err(OutOfLog));

        meta_and_logs.backward_log(Ok([6, 7, 8, 9]));
        meta_and_logs.backward_log(Ok([3, 4, 5]));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(Ok([10]), 1);

        // amount of transitions is stored as u8, cannot store more than 255 transitions per push
        meta_and_logs.forward(Err([11; 256]), 1);
    }
}
