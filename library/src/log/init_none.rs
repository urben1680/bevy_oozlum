use std::{
    collections::{vec_deque::Drain, TryReserveError, VecDeque},
    mem::take,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{LoggedAt, OutOfLog, StateLog};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InitNoneLog<T>(Inner<T>);

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Inner<T> {
    NeverRan {
        empty: VecDeque<T>,
    },
    Ran {
        log: StateLog<T>,
        /// If `None`, own none state is out of log or was never init as none.
        /// For simplicity, this never gets unset by `pop`/`drain_past_by_len`.
        undone_first_run: Option<bool>,
    },
}

#[cfg(feature = "serde")]
mod serde_with {
    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessState, LoglessWithCapacity, WithCapacity};

    use super::*;

    #[derive(serde::Serialize, serde::Deserialize)]
    pub(crate) enum InnerWithCapacity<T> {
        NeverRan {
            capacity: usize,
        },
        Ran {
            log: T,
            undone_first_run: Option<bool>,
        },
    }

    impl<T> LoglessState for InitNoneLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = Option<&'se T>;
        type De = Option<T>;
        fn get_logless_state(&self) -> Self::Se<'_> {
            self.get()
        }
        fn from_logless_state(logless_state: Self::De) -> Self {
            logless_state.into()
        }
    }

    impl<T> WithCapacity for InitNoneLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = InnerWithCapacity<<StateLog<T> as WithCapacity>::Se<'se>>;
        type De = InnerWithCapacity<<StateLog<T> as WithCapacity>::De>;
        fn get_with_capacity(&self) -> Self::Se<'_> {
            match self.0 {
                Inner::NeverRan { ref empty } => InnerWithCapacity::NeverRan {
                    capacity: empty.capacity(),
                },
                Inner::Ran {
                    ref log,
                    undone_first_run,
                } => InnerWithCapacity::Ran {
                    log: log.get_with_capacity(),
                    undone_first_run,
                },
            }
        }
        fn from_with_capacity(with_capacity: Self::De) -> Self {
            Self(match with_capacity {
                InnerWithCapacity::NeverRan { capacity } => Inner::NeverRan {
                    empty: VecDeque::with_capacity(capacity),
                },
                InnerWithCapacity::Ran {
                    log,
                    undone_first_run,
                } => Inner::Ran {
                    log: StateLog::from_with_capacity(log),
                    undone_first_run,
                },
            })
        }
    }

    impl<T> LoglessWithCapacity for InitNoneLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (Option<&'se T>, usize);
        type De = (Option<T>, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            match self.0 {
                Inner::NeverRan { ref empty } => (None, empty.capacity()),
                Inner::Ran {
                    ref log,
                    undone_first_run: Some(true),
                } => (None, log.states_capacity()),
                Inner::Ran { ref log, .. } => (Some(&*log), log.states_capacity()),
            }
        }
        fn from_logless_with_capacity((state, capacity): Self::De) -> Self {
            Self(match state {
                Some(present) => Inner::Ran {
                    log: StateLog::with_capacity(present, capacity),
                    undone_first_run: None,
                },
                None => Inner::NeverRan {
                    empty: VecDeque::with_capacity(capacity),
                },
            })
        }
    }
}

impl<T> Default for InitNoneLog<T> {
    fn default() -> Self {
        Self::new_none()
    }
}

impl<T> From<T> for InitNoneLog<T> {
    fn from(present: T) -> Self {
        Self::new_some(present)
    }
}

impl<T> From<Option<T>> for InitNoneLog<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(present) => Self::new_some(present),
            None => Self::new_none(),
        }
    }
}

impl<T: PartialEq> PartialEq<Option<T>> for InitNoneLog<T> {
    fn eq(&self, other: &Option<T>) -> bool {
        self.get() == other.as_ref()
    }
}

impl<T: PartialEq> PartialEq<Option<&T>> for InitNoneLog<T> {
    fn eq(&self, other: &Option<&T>) -> bool {
        self.get() == *other
    }
}

impl<T> InitNoneLog<T> {
    pub const fn new_none() -> Self {
        Self(Inner::NeverRan {
            empty: VecDeque::new(),
        })
    }
    pub const fn new_some(present: T) -> Self {
        Self(Inner::Ran {
            log: StateLog::new(present),
            undone_first_run: None,
        })
    }
    pub fn none_with_capacity(states_capacity: usize) -> Self {
        Self(Inner::NeverRan {
            empty: VecDeque::with_capacity(states_capacity),
        })
    }
    pub fn some_with_capacity(present: T, states_capacity: usize) -> Self {
        Self(Inner::Ran {
            log: StateLog::with_capacity(present, states_capacity),
            undone_first_run: None,
        })
    }
    pub fn into_inner(self) -> Option<T> {
        match self.0 {
            Inner::NeverRan { .. } => None,
            Inner::Ran {
                undone_first_run: Some(true),
                ..
            } => None,
            Inner::Ran { log, .. } => Some(log.into_inner()),
        }
    }
    pub fn get(&self) -> Option<&T> {
        match self.0 {
            Inner::Ran {
                ref log,
                undone_first_run,
            } if undone_first_run != Some(true) => Some(&*log),
            _ => None,
        }
    }
    pub fn states_len(&self) -> usize {
        match &self.0 {
            Inner::NeverRan { .. } => 0,
            Inner::Ran { log, .. } => log.states_len(),
        }
    }
    pub fn states_capacity(&self) -> usize {
        match &self.0 {
            Inner::NeverRan { empty } => empty.capacity(),
            Inner::Ran { log, .. } => log.states_capacity(),
        }
    }
    pub fn states_is_empty(&self) -> bool {
        match &self.0 {
            Inner::NeverRan { .. } => true,
            Inner::Ran { log, .. } => log.states_is_empty(),
        }
    }
    pub fn states_reserve(&mut self, additional: usize) {
        match &mut self.0 {
            Inner::NeverRan { empty } => empty.reserve(additional),
            Inner::Ran { log, .. } => log.states_reserve(additional),
        }
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        match &mut self.0 {
            Inner::NeverRan { empty } => empty.reserve_exact(additional),
            Inner::Ran { log, .. } => log.states_reserve_exact(additional),
        }
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match &mut self.0 {
            Inner::NeverRan { empty } => empty.try_reserve(additional),
            Inner::Ran { log, .. } => log.states_try_reserve(additional),
        }
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match &mut self.0 {
            Inner::NeverRan { empty } => empty.try_reserve_exact(additional),
            Inner::Ran { log, .. } => log.states_try_reserve_exact(additional),
        }
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        match &mut self.0 {
            Inner::NeverRan { empty } => empty.shrink_to(min_capacity),
            Inner::Ran { log, .. } => log.states_shrink_to(min_capacity),
        }
    }
    pub fn states_shrink_to_fit(&mut self) {
        match &mut self.0 {
            Inner::NeverRan { empty } => empty.shrink_to_fit(),
            Inner::Ran { log, .. } => log.states_shrink_to_fit(),
        }
    }
    pub fn push_present(&mut self, state: T) {
        match &mut self.0 {
            Inner::NeverRan { .. } => self.never_ran_push(state),
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                if *undone_first_run == Some(true) {
                    log.clear_with(state);
                    *undone_first_run = Some(false);
                } else {
                    log.push_present(state)
                }
            }
        }
    }
    pub fn drain_future(&mut self) -> Drain<T> {
        if matches!(
            self.0,
            Inner::Ran {
                undone_first_run: Some(true),
                ..
            }
        ) {
            let (oldest, mut log) = match take(self).0 {
                Inner::Ran { log, .. } => log.into_inner_with_log(),
                Inner::NeverRan { .. } => unreachable!(),
            };
            log.reserve_exact(1);
            log.push_front(oldest);
            self.0 = Inner::NeverRan { empty: log };
            return match &mut self.0 {
                Inner::NeverRan { empty } => empty.drain(..),
                Inner::Ran { .. } => unreachable!(),
            };
        }
        match &mut self.0 {
            Inner::NeverRan { empty } => empty.drain(..0),
            Inner::Ran { log, .. } => log.drain_future(),
        }
    }
    pub fn clear_log(&mut self) {
        if let Inner::Ran {
            log,
            undone_first_run,
        } = &mut self.0
        {
            if *undone_first_run == Some(true) {
                self.clear()
            } else {
                log.clear();
                *undone_first_run = None;
            }
        }
    }
    pub fn clear(&mut self) {
        let empty = match take(self).0 {
            Inner::NeverRan { empty } => empty,
            Inner::Ran { log, .. } => log.into_inner_with_log().1,
        };
        self.0 = Inner::NeverRan { empty };
    }
    pub fn clear_with(&mut self, present: T) {
        match &mut self.0 {
            Inner::NeverRan { .. } => self.never_ran_push(present),
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                log.clear_with(present);
                *undone_first_run = None;
            }
        }
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        match self.0 {
            Inner::Ran {
                undone_first_run: Some(true),
                ..
            }
            | Inner::NeverRan { .. } => Err(OutOfLog),
            Inner::Ran {
                ref mut log,
                undone_first_run: None,
            } => log.backward_log(),
            Inner::Ran {
                ref mut log,
                ref mut undone_first_run,
            } => {
                *undone_first_run = Some(log.backward_log() == Err(OutOfLog));
                Ok(())
            }
        }
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        match self.0 {
            Inner::Ran {
                undone_first_run: Some(ref mut undone),
                ..
            } if *undone => {
                *undone = false;
                Ok(())
            }
            Inner::Ran { ref mut log, .. } => log.forward_log(),
            Inner::NeverRan { .. } => Err(OutOfLog),
        }
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        match self.0 {
            Inner::Ran {
                undone_first_run: Some(true),
                ..
            }
            | Inner::NeverRan { .. } => None,
            Inner::Ran {
                ref mut log,
                ref mut undone_first_run,
            } => log
                .pop_past_by_len(max_past_len)
                .inspect(|_| *undone_first_run = None),
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> Drain<T> {
        match self.0 {
            Inner::NeverRan { ref mut empty } => empty.drain(..0),
            Inner::Ran {
                ref mut log,
                undone_first_run: Some(true),
            } => log.drain_past_by_len(usize::MAX),
            Inner::Ran {
                ref mut log,
                ref mut undone_first_run,
            } => {
                let iter = log.drain_past_by_len(max_past_len);
                if iter.len() != 0 {
                    *undone_first_run = None;
                }
                iter
            }
        }
    }
    fn never_ran_push(&mut self, present: T) {
        let empty = match take(self).0 {
            Inner::NeverRan { empty } => empty,
            Inner::Ran { .. } => unreachable!(),
        };
        self.0 = Inner::Ran {
            log: StateLog::with_alloc(present, empty),
            undone_first_run: Some(false),
        };
    }
}

impl<T: LoggedAt> InitNoneLog<T> {
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<T> {
        match self.0 {
            Inner::Ran {
                undone_first_run: Some(true),
                ..
            }
            | Inner::NeverRan { .. } => None,
            Inner::Ran {
                ref mut log,
                ref mut undone_first_run,
            } => log
                .pop_past_by_logged_at(meta)
                .inspect(|_| *undone_first_run = None),
        }
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> Drain<T> {
        match self.0 {
            Inner::NeverRan { ref mut empty } => empty.drain(..0),
            Inner::Ran {
                ref mut log,
                undone_first_run: Some(true),
            } => log.drain_past_by_len(usize::MAX),
            Inner::Ran {
                ref mut log,
                ref mut undone_first_run,
            } => {
                let iter = log.truncate_future_drain_past_by_logged_at(meta);
                if iter.len() != 0 {
                    *undone_first_run = None;
                }
                iter
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::{
        log::test::{shorten_strategy, ShortenStrategy},
        meta::RevMeta,
        RevFrame,
    };

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: InitNoneLog<char>,
            #[serde(with = "crate::log::logless_state")]
            logless: InitNoneLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: InitNoneLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: InitNoneLog<char>,
        }

        impl Logs {
            fn new(log: InitNoneLog<char>, reserve_additional: usize) -> Self {
                let mut logs = Self {
                    full: log.clone(),
                    logless: log.clone(),
                    full_with_capacity: log.clone(),
                    logless_with_capacity: log.clone(),
                };
                logs.full.states_reserve_exact(reserve_additional);
                logs.logless.states_reserve_exact(reserve_additional);
                logs.full_with_capacity
                    .states_reserve_exact(reserve_additional);
                logs.logless_with_capacity
                    .states_reserve_exact(reserve_additional);
                logs
            }
        }

        #[derive(Serialize, Deserialize)]
        struct LogsIn {
            never_ran: Logs,
            ran_after_none: Logs,
            undone_first_run: Logs,
            never_none: Logs,
        }

        let original_never_ran = InitNoneLog::new_none();
        assert!(matches!(original_never_ran.0, Inner::NeverRan { .. }));

        let mut original_ran_after_none = original_never_ran.clone();
        original_ran_after_none.push_present('a');
        original_ran_after_none.push_present('b');
        original_ran_after_none.push_present('c');
        original_ran_after_none.backward_log().expect("in log");
        assert!(matches!(
            original_ran_after_none.0,
            Inner::Ran {
                undone_first_run: Some(false),
                ..
            }
        ));

        let mut original_undone_first_run = original_ran_after_none.clone();
        original_undone_first_run.backward_log().expect("in log");
        original_undone_first_run.backward_log().expect("in log");
        assert!(matches!(
            original_undone_first_run.0,
            Inner::Ran {
                undone_first_run: Some(true),
                ..
            }
        ));

        let mut original_never_none = original_ran_after_none.clone();
        original_never_none.pop_past_by_len(1);
        assert!(matches!(
            original_never_none.0,
            Inner::Ran {
                undone_first_run: None,
                ..
            }
        ));

        let logs = LogsIn {
            never_ran: Logs::new(original_never_ran.clone(), 100),
            ran_after_none: Logs::new(original_ran_after_none.clone(), 98),
            undone_first_run: Logs::new(original_undone_first_run.clone(), 98),
            never_none: Logs::new(original_never_none.clone(), 98),
        };

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let LogsIn {
            never_ran,
            ran_after_none,
            undone_first_run,
            never_none,
        } = serde_json::from_str(&serialized).unwrap();

        struct Test {
            logs: Logs,
            original: InitNoneLog<char>,
            expected_state: Option<char>,
            expected_len: usize,
            name: &'static str,
        }

        let tests = [
            Test {
                logs: never_ran,
                original: original_never_ran,
                expected_state: None,
                expected_len: 0,
                name: "never_ran",
            },
            Test {
                logs: ran_after_none,
                original: original_ran_after_none,
                expected_state: Some('b'),
                expected_len: 2,
                name: "ran_after_none",
            },
            Test {
                logs: undone_first_run,
                original: original_undone_first_run,
                expected_state: None,
                expected_len: 2,
                name: "undone_first_run",
            },
            Test {
                logs: never_none,
                original: original_never_none,
                expected_state: Some('b'),
                expected_len: 2,
                name: "never_none",
            },
        ];

        for test in tests {
            let Test {
                logs,
                original,
                expected_state,
                expected_len,
                name,
            } = test;

            let Logs {
                full,
                logless,
                full_with_capacity,
                logless_with_capacity,
            } = logs;

            let test = |log: &InitNoneLog<char>, expected_len, with_capacity| {
                assert_eq!(
                    log.get().cloned(), expected_state,
                    "name: {name}\nbefore: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
                );
                assert_eq!(
                    log.states_len(),
                    expected_len,
                    "name: {name}\nbefore: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
                );
                assert_eq!(
                    log.states_capacity() >= 100,
                    with_capacity,
                    "name: {name}\nbefore: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                    log.states_capacity()
                );
            };

            test(&full, expected_len, false);
            test(&logless, 0, false);
            test(&full_with_capacity, expected_len, true);
            test(&logless_with_capacity, 0, true);
        }
    }

    impl InitNoneLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            push: u8,
            expected_states_len: usize,
            expected_popped: Option<(u8, usize)>,
        ) {
            meta.queue_forward();
            meta.update(|_, _| {});
            let before = self.clone();
            let push = (push, meta.present_world_state());
            self.push_present(push);
            let after_push = self.clone();
            let actual_popped = shorten_strategy!(
                self,
                meta,
                strategy,
                meta.past_world_states(),
                before,
                after_push
            );
            assert_eq!(
                actual_popped, expected_popped,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.states_len(),
                expected_states_len,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.get().cloned(), Some(push),
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: Option<u8>,
            out_of_log: bool,
        ) {
            let before = self.clone();
            if out_of_log {
                let result = self.forward_log();
                assert_eq!(
                    result,
                    Err(OutOfLog),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            } else {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap_or_else(|_| {
                    panic!("\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}")
                });
                meta.update(|_, _| {});
                let result = self.forward_log();
                assert_eq!(
                    result,
                    Ok(()),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            }
            self.test_state(before, meta, expected_state);
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: Option<u8>,
            out_of_log: bool,
        ) {
            let before = self.clone();
            if out_of_log {
                let result = self.backward_log();
                assert_eq!(
                    result,
                    Err(OutOfLog),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            } else {
                let frame = meta.present_world_state().wrapping_sub(1);
                meta.queue_log(frame).unwrap_or_else(|_| {
                    panic!("\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}")
                });
                meta.update(|_, _| {});
                let result = self.backward_log();
                assert_eq!(
                    result,
                    Ok(()),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            }
            self.test_state(before, meta, expected_state);
        }
        fn test_state(&self, before: Self, meta: &RevMeta, expected_state: Option<u8>) {
            let expected_state =
                expected_state.map(|expected_state| (expected_state, meta.present_world_state()));
            assert_eq!(
                self.get().cloned(),
                expected_state,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (u8, usize)>,
            expected_states_len: usize,
        ) -> Self {
            let before = self.clone();
            let mut clone = self.clone();
            let actual_future: Vec<_> = clone.drain_future().collect();
            let expected_future: Vec<_> = expected_future
                .into_iter()
                .map(|(state, frame)| (state, RevFrame(frame)))
                .collect();
            assert_eq!(
                actual_future, expected_future,
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            assert_eq!(
                clone.states_len(),
                expected_states_len,
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroUsize::new(3), 0, false);
            let mut init_none = InitNoneLog::new_none();

            init_none.test_forward(meta, strategy, 1, 0, None);

            // reaching pre-state None
            init_none.test_backward_log(meta, None, false);
            // out of log, no mutations happend to both meta and log here
            init_none.test_backward_log(meta, None, true);

            // back in state Some
            init_none.test_forward_log(meta, Some(1), false);
            // out of log, no mutations happend to both meta and log here
            init_none.test_forward_log(meta, Some(1), true);

            init_none.test_forward(meta, strategy, 2, 1, None);

            // has no initial value that is out of log now
            init_none.test_forward(meta, strategy, 3, 2, None);

            let meta = &mut RevMeta::new(NonZeroUsize::new(3), 0, false);
            let mut init_some = InitNoneLog::new_some((0, meta.present_world_state()));

            init_some.test_forward(meta, strategy, 1, 1, None);

            init_some.test_backward_log(meta, Some(0), false);
            // out of log, no mutations happend to both meta and log here
            init_some.test_backward_log(meta, Some(0), true);

            init_some.test_forward_log(meta, Some(1), false);
            // out of log, no mutations happend to both meta and log here
            init_some.test_forward_log(meta, Some(1), true);

            init_some.test_forward(meta, strategy, 2, 2, None);

            // pops the initial value that became out-of-log
            init_some.test_forward(meta, strategy, 3, 2, Some((0, 0)));

            for mut log in [init_none, init_some] {
                let meta = &mut meta.clone();

                // reduces log, this does pop a state
                log.test_forward(meta, strategy, 4, 2, Some((1, 1)));

                log.test_backward_log(meta, Some(3), false);
                log.test_backward_log(meta, Some(2), false);
                // out of log, no mutations happend to both meta and log here
                log.test_backward_log(meta, Some(2), true);

                log.test_forward_log(meta, Some(3), false);
                log.test_forward_log(meta, Some(4), false);
                // out of log, no mutations happend to both meta and log here
                log.test_forward_log(meta, Some(4), true);

                log.test_backward_log(meta, Some(3), false);
                log.test_backward_log(meta, Some(2), false);

                let clone = log.test_drain_future([(3, 3), (4, 4)], 0);

                for mut log in [log, clone] {
                    // all entries are truncated as they are in the future
                    log.test_forward(meta, strategy, 5, 1, None);
                }
            }
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<InitNoneLog<RevFrame>>();
    }
}
