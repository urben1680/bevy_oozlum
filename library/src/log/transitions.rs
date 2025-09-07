use std::{
    collections::{
        VecDeque,
        vec_deque::{Drain, IterMut},
    },
    fmt::Debug,
    marker::PhantomData,
};

use crate::{
    log::{
        PreUpdateVariant, TransitionDrain, TransitionDrainFuture, TransitionDrainPast,
        transition::TransitionDrains,
    },
    meta::RevMeta,
};

use super::{OutOfLog, TransitionLog};

#[derive(Debug)]
pub struct TransitionsLog<T, U = ()> {
    amounts: TransitionLog<EntryAmount<U>>,
    transitions: VecDeque<T>,
    index: usize,
}

pub struct TransitionsDrains<'log, T, U> {
    transitions: Drain<'log, T>,
    past_len: usize,
    entry_amounts: TransitionDrains<'log, EntryAmount<U>>,
}

pub type TransitionsDrainPast<'a, 'log, T, U> = TransitionsDrainChunkable<
    TransitionDrainPast<'a, 'log, T>,
    TransitionDrainPast<'a, 'log, EntryAmount<U>>,
    U,
>;
pub type TransitionsDrainFuture<'log, T, U> = TransitionsDrainChunkable<
    TransitionDrainFuture<'log, T>,
    TransitionDrainFuture<'log, EntryAmount<U>>,
    U,
>;
pub type TransitionsDrain<'log, T, U> =
    TransitionsDrainChunkable<TransitionDrain<'log, T>, TransitionDrain<'log, EntryAmount<U>>, U>;

impl<'log, T, U> TransitionsDrains<'log, T, U> {
    pub fn past<'a>(&'a mut self) -> TransitionsDrainPast<'a, 'log, T, U> {
        let past_len = self.past_len;
        self.past_len = 0;
        TransitionsDrainChunkable {
            transitions: self.transitions.by_ref().take(past_len),
            entry_amounts: self.entry_amounts.past(),
            _p: PhantomData,
        }
    }
    pub fn future(self) -> TransitionsDrainFuture<'log, T, U> {
        TransitionsDrainChunkable {
            transitions: self.transitions.skip(self.past_len),
            entry_amounts: self.entry_amounts.future(),
            _p: PhantomData,
        }
    }
    pub fn all(self) -> TransitionsDrain<'log, T, U> {
        TransitionsDrainChunkable {
            transitions: self.transitions,
            entry_amounts: self.entry_amounts.all(),
            _p: PhantomData,
        }
    }
}

pub struct TransitionsDrainChunkable<TI, UI, U> {
    pub transitions: TI,
    pub entry_amounts: UI,
    _p: PhantomData<fn(UI) -> U>,
}

impl<TI, UI, U> TransitionsDrainChunkable<TI, UI, U>
where
    TI: Iterator,
    UI: Iterator<Item = EntryAmount<U>>,
{
    pub fn next_chunk<'a>(&'a mut self) -> Option<(core::iter::Take<&'a mut TI>, U)> {
        self.entry_amounts.next().map(|entry_amount| {
            (
                self.transitions.by_ref().take(entry_amount.amount),
                entry_amount.entry,
            )
        })
    }
}

impl<T, U> Default for TransitionsLog<T, U> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U> TransitionsLog<T, U> {
    pub const fn new() -> Self {
        Self {
            amounts: TransitionLog::new(),
            transitions: VecDeque::new(),
            index: 0,
        }
    }
    pub fn push_and_truncate_past<IntoU: Into<U>>(
        &mut self,
        max_past_len: u64,
        c: impl FnOnce(LogMut<T>) -> IntoU,
    ) {
        let (transition_drain, amount_drain) =
            self.push_and_get_transition_and_amount_drain(max_past_len, c);
        // todo: truncate_front https://github.com/rust-lang/rust/issues/140667
        self.transitions.drain(..transition_drain);
        self.amounts.drain_past(amount_drain);
    }
    pub fn push_and_drain_past<IntoU: Into<U>>(
        &mut self,
        max_past_len: u64,
        c: impl FnOnce(LogMut<T>) -> IntoU,
    ) -> TransitionsDrain<T, U> {
        // for the + 1, see the comment in TransitionLog::push_and_get_drain
        let (transition_drain, amount_drain) =
            self.push_and_get_transition_and_amount_drain(max_past_len + 1, c);
        TransitionsDrainChunkable {
            transitions: self.transitions.drain(..transition_drain),
            entry_amounts: self.amounts.drain_past(amount_drain),
            _p: PhantomData,
        }
    }
    fn push_and_get_transition_and_amount_drain<IntoU: Into<U>>(
        &mut self,
        max_past_len: u64,
        c: impl FnOnce(LogMut<T>) -> IntoU,
    ) -> (usize, usize) {
        assert_eq!(self.index, self.transitions.len()); // do not truncate here, call pre_update!
        let entry = c(LogMut(&mut self.transitions)).into();
        let pushed_amount = self.transitions.len() - self.index;
        let entry_amount = EntryAmount {
            entry,
            amount: pushed_amount,
        };
        self.index = self.transitions.len();
        let to_drain = self
            .amounts
            .push_and_iter_to_drain_past(max_past_len, entry_amount);
        let amount_drain = to_drain.len();
        let transition_drain: usize = to_drain.map(|entry_amount| entry_amount.amount).sum();
        self.index -= transition_drain;
        (transition_drain, amount_drain)
    }
    pub fn backward_log(&mut self) -> Result<ValueEntry<IterMut<T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.backward_log()?;
        self.index -= entry_amount.amount;
        let iter = self.transitions.range_mut(self.index..old_index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
    }
    pub fn forward_log(&mut self) -> Result<ValueEntry<IterMut<T>, &mut U>, OutOfLog> {
        let old_index = self.index;
        let entry_amount = self.amounts.forward_log()?;
        self.index += entry_amount.amount;
        let iter = self.transitions.range_mut(old_index..self.index);
        Ok(ValueEntry {
            value: iter,
            entry: &mut entry_amount.entry,
        })
    }
    pub fn pre_update(&mut self, meta: &RevMeta) {
        match self.amounts.pre_update_check(meta) {
            PreUpdateVariant::RemoveLog => {
                self.transitions.clear();
                self.amounts.clear();
                self.index = 0;
            }
            PreUpdateVariant::RemoveFuture => {
                self.transitions.truncate(self.index);
                self.amounts.truncate_future();
            }
            PreUpdateVariant::Nothing => {}
        }
    }
    pub fn pre_update_drain<'log, 'm>(
        &'log mut self,
        meta: &'m RevMeta,
    ) -> TransitionsDrains<'log, T, U> {
        match self.amounts.pre_update_check(meta) {
            PreUpdateVariant::RemoveLog => {
                let past_len = self.index;
                self.index = 0;
                TransitionsDrains {
                    transitions: self.transitions.drain(..),
                    past_len,
                    entry_amounts: self.amounts.full_drain(),
                }
            }
            PreUpdateVariant::RemoveFuture => TransitionsDrains {
                transitions: self.transitions.drain(self.index..),
                past_len: 0,
                entry_amounts: self.amounts.drain_future(),
            },
            PreUpdateVariant::Nothing => TransitionsDrains {
                transitions: self.transitions.drain(..0),
                past_len: 0,
                entry_amounts: self.amounts.empty_drain(),
            },
        }
    }
}

/// A [`&mut VecDeque<T>`](VecDeque) wrapper that does not expose methods which remove from the
/// deque.
pub struct LogMut<'a, T>(&'a mut VecDeque<T>);

impl<'a, T> LogMut<'a, T> {
    pub fn append(&mut self, other: &mut VecDeque<T>) {
        self.0.append(other);
    }
    pub fn push(&mut self, value: T) {
        self.0.push_back(value);
    }
}

impl<'a, T> Extend<T> for LogMut<'a, T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<'a, T> Extend<&'a T> for LogMut<'a, T>
where
    T: 'a + Copy,
{
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueEntry<T, U> {
    pub value: T,
    pub entry: U,
}

impl<'a, T: Iterator, U> IntoIterator for &'a mut ValueEntry<T, U> {
    type IntoIter = &'a mut T;
    type Item = T::Item;
    fn into_iter(self) -> Self::IntoIter {
        &mut self.value
    }
}

/// `EntryAmount` is usually encountered in draining methods of logs with multiple states/transitions per update,
/// for example [`DenseStatesLog`] which will be behind the `log` variable in the following code snippets.
///
/// These methods return two draining iterators, the first with the states/transitions of all updates that are
/// drained, and the second with `EntryAmount`, containing the entry type `U` of the log (if specified) and the
/// amount of states/transitions per update, returned by the [`amount`](Self::amount) method.
#[derive(Debug, Clone)]
pub struct EntryAmount<U> {
    pub entry: U,

    /// The amount of transitions of an update. This can be useful to chunk them.
    ///
    /// # Examples
    ///
    /// With `log` being [`&mut DenseStatesLog`](DenseStatesLog) and [`DenseStatesLog::drain_future`] returning the
    /// draining iterators, do this to chunk them by updates:
    ///
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// let (mut future_states, future_entry_amounts) = log.drain_future();
    /// ```
    ///
    /// Now iterate...
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// for entry_amount in future_entry_amounts {
    ///     let entry = entry_amount.entry;
    ///     for future_states in future_states.by_ref().take(entry_amount.amount()) {
    ///         // logic
    ///     }
    /// }
    /// ```
    ///
    /// ...or collect them:
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// let updates: Vec<(Vec<_>, _)> = future_entry_amounts.map(|entry_amount| (
    ///     future_states.by_ref().take(entry_amount.amount()).collect(),
    ///     entry_amount.entry
    /// )).collect();
    /// ```
    pub amount: usize,
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU64;

    use crate::meta::{RevDirection, RevQueue};

    use super::*;

    struct MetaAndLogs {
        meta: RevMeta,
        with_past_drain: TransitionsLog<char, char>,
        without_past_drain: TransitionsLog<char, char>,
    }

    impl<TI, UI> TransitionsDrainChunkable<TI, UI, char>
    where
        TI: Iterator<Item = char>,
        UI: Iterator<Item = EntryAmount<char>>,
    {
        fn to_tuples(mut self) -> Vec<(String, char)> {
            let mut v = Vec::new();
            while let Some((transitions, entry)) = self.next_chunk() {
                v.push((transitions.collect(), entry))
            }
            v
        }
    }

    impl<'a> ValueEntry<IterMut<'a, char>, &'a mut char> {
        fn to_tuple(self) -> (String, char) {
            (self.value.map(|char| *char).collect(), *self.entry)
        }
    }

    impl MetaAndLogs {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(NonZeroU64::new(max_world_states), false),
                with_past_drain: TransitionsLog::new(),
                without_past_drain: TransitionsLog::new(),
            }
        }
        fn forward<const N: usize, const M: usize>(
            &mut self,
            past_drain: [(String, char); N],
            future_drain: [(String, char); M],
            push: (String, char),
            clear: bool,
        ) {
            let queue = if clear {
                RevQueue::CLEAR_THEN_RUN
            } else {
                RevQueue::RUN_NOT_LOG
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::NOT_LOG);

                // with_past_drain
                let mut drain = self.with_past_drain.pre_update_drain(meta);
                if clear {
                    assert_eq!(drain.past().to_tuples(), past_drain);
                } else {
                    assert_eq!(drain.past().to_tuples(), []);
                }
                assert_eq!(drain.future().to_tuples(), future_drain);
                let drain = self
                    .with_past_drain
                    .push_and_drain_past(meta.past_len(), |mut log| {
                        log.extend(push.0.chars());
                        push.1
                    });
                if clear {
                    assert_eq!(drain.to_tuples(), []);
                } else {
                    assert_eq!(drain.to_tuples(), past_drain)
                }

                // without_past_drain
                let drain = self.without_past_drain.pre_update_drain(meta);
                assert_eq!(drain.future().to_tuples(), future_drain);
                self.without_past_drain
                    .push_and_truncate_past(meta.past_len(), |mut log| {
                        log.extend(push.0.chars());
                        push.1
                    });
            });
        }
        fn noop_forward_backward_log(&mut self) {
            self.meta.set_queue(RevQueue::RUN_NOT_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |_, _| ());
        }
        fn forward_log(&mut self, get: Result<(String, char), OutOfLog>) {
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            match get {
                Ok(get) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);

                        // with_past_drain
                        let drain = self.with_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.all().to_tuples(), []);
                        assert_eq!(
                            self.with_past_drain.forward_log().map(ValueEntry::to_tuple),
                            Ok(get.clone())
                        );

                        // without_past_drain
                        let drain = self.without_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.future().to_tuples(), []);
                        assert_eq!(
                            self.without_past_drain
                                .forward_log()
                                .map(ValueEntry::to_tuple),
                            Ok(get)
                        );
                    });
                }
                Err(OutOfLog) => {
                    self.meta.update_ref(Ok(false), |_, _| ());
                    assert_eq!(
                        self.with_past_drain.forward_log().map(ValueEntry::to_tuple),
                        Err(OutOfLog)
                    );
                    assert_eq!(
                        self.without_past_drain
                            .forward_log()
                            .map(ValueEntry::to_tuple),
                        Err(OutOfLog)
                    );
                }
            }
        }
        fn backward_log<const N: usize>(
            &mut self,
            future_drain: [(String, char); N],
            get: Result<(String, char), OutOfLog>,
        ) {
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            match get {
                Ok(get) => {
                    self.meta.update_ref(Ok(true), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);

                        // with_past_drain
                        let mut drain = self.with_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.past().to_tuples(), []);
                        assert_eq!(drain.future().to_tuples(), future_drain);
                        assert_eq!(
                            self.with_past_drain
                                .backward_log()
                                .map(ValueEntry::to_tuple),
                            Ok(get.clone())
                        );

                        // without_past_drain
                        let drain = self.without_past_drain.pre_update_drain(meta);
                        assert_eq!(drain.future().to_tuples(), future_drain);
                        assert_eq!(
                            self.without_past_drain
                                .backward_log()
                                .map(ValueEntry::to_tuple),
                            Ok(get)
                        );
                    });
                }
                Err(OutOfLog) => {
                    assert_eq!(N, 0);
                    self.meta.update_ref(Ok(false), |_, _| ());

                    // with_past_drain
                    if self.with_past_drain.backward_log().is_ok() {
                        // Because this past-draining log secretly keeps one more transition than
                        // needed, OutOfLog will only be triggered if going backward twice past what
                        // the user might suspect to be in log.
                        // This is not true when clearing is involved however, which is why the
                        // first backward_log is not asserted to be Ok here.

                        // test again
                        assert_eq!(
                            self.with_past_drain
                                .backward_log()
                                .map(ValueEntry::to_tuple),
                            Err(OutOfLog)
                        );

                        // undoing first backward_log
                        assert!(self.with_past_drain.forward_log().is_ok());
                    }

                    // without_past_drain
                    assert_eq!(
                        self.without_past_drain
                            .backward_log()
                            .map(ValueEntry::to_tuple),
                        Err(OutOfLog)
                    );
                }
            }
        }
    }

    #[test]
    fn traverses_log() {
        let mut meta_and_logs = MetaAndLogs::new(5);

        let a = || ("a".repeat(1), 'A');
        let b = || ("b".repeat(2), 'B');
        let c = || ("c".repeat(3), 'C');
        let d = || ("d".repeat(4), 'D');
        let e = || ("e".repeat(5), 'E');
        let f = || ("f".repeat(6), 'F');
        let g = || ("g".repeat(7), 'G');
        let h = || ("h".repeat(8), 'H');
        let i = || ("i".repeat(9), 'I');

        meta_and_logs.forward([], [], a(), false);
        meta_and_logs.forward([], [], b(), false);
        meta_and_logs.forward([], [], c(), false);
        meta_and_logs.forward([], [], d(), false);
        meta_and_logs.forward([], [], e(), false);
        meta_and_logs.forward([a()], [], f(), false);

        meta_and_logs.backward_log([], Ok(f()));
        meta_and_logs.backward_log([], Ok(e()));
        meta_and_logs.backward_log([], Ok(d()));
        meta_and_logs.backward_log([], Ok(c()));
        meta_and_logs.backward_log([], Err(OutOfLog)); // b() is unreachable but not yet drained

        meta_and_logs.forward_log(Ok(c()));
        meta_and_logs.forward_log(Ok(d()));
        meta_and_logs.forward_log(Ok(e()));
        meta_and_logs.forward_log(Ok(f()));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log([], Ok(f()));
        meta_and_logs.backward_log([], Ok(e()));

        meta_and_logs.forward([], [e(), f()], g(), false);

        meta_and_logs.backward_log([], Ok(g()));
        meta_and_logs.backward_log([], Ok(d()));
        meta_and_logs.backward_log([], Ok(c()));
        meta_and_logs.backward_log([], Err(OutOfLog));

        meta_and_logs.forward_log(Ok(c()));
        meta_and_logs.forward_log(Ok(d()));
        meta_and_logs.forward_log(Ok(g()));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log([], Ok(g()));
        meta_and_logs.backward_log([], Ok(d()));

        meta_and_logs.forward([b(), c()], [d(), g()], h(), true);

        meta_and_logs.backward_log([], Ok(h()));
        meta_and_logs.backward_log([], Err(OutOfLog));

        meta_and_logs.forward_log(Ok(h()));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.forward([], [], i(), false);

        meta_and_logs.backward_log([], Ok(i()));

        meta_and_logs.noop_forward_backward_log();

        meta_and_logs.backward_log([i()], Ok(h()));
        meta_and_logs.backward_log([], Err(OutOfLog));
    }
}
