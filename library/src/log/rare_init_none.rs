use std::{
    collections::{TryReserveError, VecDeque},
    mem::take,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{LoggedAt, OutOfLog, RareDrain, RareStateLog, RareValue};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareInitNoneLog<T>(Inner<T>);

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Inner<T> {
    NeverRan {
        empty: VecDeque<RareValue<T>>,
        skips: usize,
        /// For simplicity, this never gets reduced by `pop`/`drain_past_by_len`.
        skips_max: usize,
    },
    Ran {
        log: RareStateLog<T>,
        /// If `None`, own none state is out of log or was never init as none.
        undone_first_run: Option<UndoneFirstRun>,
    },
}

#[derive(Debug, Clone, Copy, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct UndoneFirstRun {
    undone: bool,
    /// For simplicity, this never gets reduced by `pop`/`drain_past_by_len`.
    skips_max: usize,
}

fn is_undone(undone_first_run: &Option<UndoneFirstRun>) -> bool {
    undone_first_run.as_ref().is_some_and(|value| value.undone)
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
            skips: usize,
            skips_max: usize,
        },
        Ran {
            log: T,
            undone_first_run: Option<UndoneFirstRun>,
        },
    }

    impl<T> LoglessState for RareInitNoneLog<T>
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

    impl<T> WithCapacity for RareInitNoneLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = InnerWithCapacity<<RareStateLog<T> as WithCapacity>::Se<'se>>;
        type De = InnerWithCapacity<<RareStateLog<T> as WithCapacity>::De>;
        fn get_with_capacity(&self) -> Self::Se<'_> {
            match self.0 {
                Inner::NeverRan {
                    ref empty,
                    skips,
                    skips_max,
                } => InnerWithCapacity::NeverRan {
                    capacity: empty.capacity(),
                    skips,
                    skips_max,
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
                InnerWithCapacity::NeverRan {
                    capacity,
                    skips,
                    skips_max,
                } => Inner::NeverRan {
                    empty: VecDeque::with_capacity(capacity),
                    skips,
                    skips_max,
                },
                InnerWithCapacity::Ran {
                    log,
                    undone_first_run,
                } => Inner::Ran {
                    log: RareStateLog::from_with_capacity(log),
                    undone_first_run,
                },
            })
        }
    }

    impl<T> LoglessWithCapacity for RareInitNoneLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (Option<&'se T>, usize);
        type De = (Option<T>, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            match &self.0 {
                Inner::NeverRan { empty, .. } => (None, empty.capacity()),
                Inner::Ran {
                    log,
                    undone_first_run,
                } if is_undone(undone_first_run) => (None, log.states_capacity()),
                Inner::Ran { ref log, .. } => (Some(&*log), log.states_capacity()),
            }
        }
        fn from_logless_with_capacity((state, capacity): Self::De) -> Self {
            Self(match state {
                Some(present) => Inner::Ran {
                    log: RareStateLog::with_capacity(present, capacity),
                    undone_first_run: None,
                },
                None => Inner::NeverRan {
                    empty: VecDeque::with_capacity(capacity),
                    skips: 0,
                    skips_max: 0,
                },
            })
        }
    }
}

impl<T> Default for RareInitNoneLog<T> {
    fn default() -> Self {
        Self::new_none()
    }
}

impl<T> From<T> for RareInitNoneLog<T> {
    fn from(present: T) -> Self {
        Self::new_some(present)
    }
}

impl<T> From<Option<T>> for RareInitNoneLog<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(present) => Self::new_some(present),
            None => Self::new_none(),
        }
    }
}

impl<T: PartialEq> PartialEq<Option<T>> for RareInitNoneLog<T> {
    fn eq(&self, other: &Option<T>) -> bool {
        self.get() == other.as_ref()
    }
}

impl<T: PartialEq> PartialEq<Option<&T>> for RareInitNoneLog<T> {
    fn eq(&self, other: &Option<&T>) -> bool {
        self.get() == *other
    }
}

impl<T> RareInitNoneLog<T> {
    pub const fn new_none() -> Self {
        Self(Inner::NeverRan {
            empty: VecDeque::new(),
            skips: 0,
            skips_max: 0,
        })
    }
    pub const fn new_some(present: T) -> Self {
        Self(Inner::Ran {
            log: RareStateLog::new(present),
            undone_first_run: None,
        })
    }
    pub fn none_with_capacity(states_capacity: usize) -> Self {
        Self(Inner::NeverRan {
            empty: VecDeque::with_capacity(states_capacity),
            skips: 0,
            skips_max: 0,
        })
    }
    pub fn some_with_capacity(present: T, states_capacity: usize) -> Self {
        Self(Inner::Ran {
            log: RareStateLog::with_capacity(present, states_capacity),
            undone_first_run: None,
        })
    }
    pub fn into_inner(self) -> Option<T> {
        match self.0 {
            Inner::NeverRan { .. } => None,
            Inner::Ran {
                ref undone_first_run,
                ..
            } if is_undone(undone_first_run) => None,
            Inner::Ran { log, .. } => Some(log.into_inner()),
        }
    }
    pub fn get(&self) -> Option<&T> {
        match &self.0 {
            Inner::Ran {
                log,
                undone_first_run,
            } if !is_undone(undone_first_run) => Some(&*log),
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
            Inner::NeverRan { empty, .. } => empty.capacity(),
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
            Inner::NeverRan { empty, .. } => empty.reserve(additional),
            Inner::Ran { log, .. } => log.states_reserve(additional),
        }
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        match &mut self.0 {
            Inner::NeverRan { empty, .. } => empty.reserve_exact(additional),
            Inner::Ran { log, .. } => log.states_reserve_exact(additional),
        }
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match &mut self.0 {
            Inner::NeverRan { empty, .. } => empty.try_reserve(additional),
            Inner::Ran { log, .. } => log.states_try_reserve(additional),
        }
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        match &mut self.0 {
            Inner::NeverRan { empty, .. } => empty.try_reserve_exact(additional),
            Inner::Ran { log, .. } => log.states_try_reserve_exact(additional),
        }
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        match &mut self.0 {
            Inner::NeverRan { empty, .. } => empty.shrink_to(min_capacity),
            Inner::Ran { log, .. } => log.states_shrink_to(min_capacity),
        }
    }
    pub fn states_shrink_to_fit(&mut self) {
        match &mut self.0 {
            Inner::NeverRan { empty, .. } => empty.shrink_to_fit(),
            Inner::Ran { log, .. } => log.states_shrink_to_fit(),
        }
    }
    pub fn push_present(&mut self, state: Option<T>) {
        match &mut self.0 {
            Inner::NeverRan { skips, .. } => {
                let skips = *skips + 1;
                self.never_ran_push(state, skips + 1)
            }
            Inner::Ran {
                log,
                undone_first_run,
            } => match undone_first_run {
                Some(UndoneFirstRun {
                    undone: true,
                    skips_max,
                }) if state.is_none() => {
                    let skips = *skips_max;
                    self.clear_with_skips(skips);
                }
                Some(UndoneFirstRun { undone, skips_max }) => {
                    log.init_none_push_present(undone, skips_max, state)
                }
                None => log.push_present(state),
            },
        }
    }
    pub fn drain_future(&mut self) -> RareDrain<T> {
        if matches!(
            &self.0,
            Inner::Ran {
                undone_first_run,
                ..
            }
            if is_undone(undone_first_run)
        ) {
            let (oldest, mut log, skips) = match take(self).0 {
                Inner::Ran { log, .. } => log.into_inner_with_log_with_skips(),
                Inner::NeverRan { .. } => unreachable!(),
            };
            log.reserve_exact(1);
            log.push_front(RareValue::new(oldest, 0)); // actual skips are not exposed anyway, would be different to `skips` here
            self.0 = Inner::NeverRan {
                empty: log,
                skips,
                skips_max: skips,
            };
            return match &mut self.0 {
                Inner::NeverRan { empty, .. } => RareDrain(empty.drain(..)),
                Inner::Ran { .. } => unreachable!(),
            };
        }
        match &mut self.0 {
            Inner::NeverRan { empty, .. } => RareDrain(empty.drain(..0)),
            Inner::Ran { log, .. } => log.drain_future(),
        }
    }
    pub fn clear_log(&mut self) {
        match &mut self.0 {
            Inner::NeverRan { skips, .. } => *skips = 0,
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                if is_undone(undone_first_run) {
                    self.clear()
                } else {
                    log.clear();
                    *undone_first_run = None;
                }
            }
        }
    }
    pub fn clear(&mut self) {
        self.clear_with_skips(0);
    }
    fn clear_with_skips(&mut self, skips: usize) {
        let empty = match take(self).0 {
            Inner::NeverRan { empty, .. } => empty,
            Inner::Ran { log, .. } => log.into_inner_with_log_with_skips().1,
        };
        self.0 = Inner::NeverRan {
            empty,
            skips,
            skips_max: skips,
        };
    }
    pub fn clear_with(&mut self, present: T) {
        match &mut self.0 {
            Inner::NeverRan { .. } => self.never_ran_push(Some(present), 0),
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                log.clear_with(present);
                *undone_first_run = None;
            }
        }
    }
    pub fn backward_log(&mut self) -> Result<bool, OutOfLog> {
        match &mut self.0 {
            Inner::Ran {
                log,
                undone_first_run,
            } => match undone_first_run.as_mut() {
                Some(UndoneFirstRun { undone, skips_max }) => {
                    log.init_none_backward_log(undone, *skips_max)
                }
                None => log.backward_log(),
            },
            Inner::NeverRan { skips, .. } => {
                *skips = skips.checked_sub(1).ok_or(OutOfLog)?;
                Ok(false)
            }
        }
    }
    pub fn forward_log(&mut self) -> Result<bool, OutOfLog> {
        match &mut self.0 {
            Inner::Ran {
                log,
                undone_first_run,
            } => match undone_first_run {
                Some(UndoneFirstRun { undone, skips_max }) => {
                    log.init_none_forward_log(undone, *skips_max)
                }
                None => log.forward_log(),
            },
            Inner::NeverRan {
                skips, skips_max, ..
            } => {
                if *skips < *skips_max {
                    *skips += 1;
                    Ok(false)
                } else {
                    Err(OutOfLog)
                }
            }
        }
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        match &mut self.0 {
            Inner::NeverRan { .. } => None,
            Inner::Ran {
                undone_first_run, ..
            } if is_undone(undone_first_run) => None,
            Inner::Ran {
                log,
                undone_first_run,
            } => log
                .pop_past_by_len(max_past_len)
                .inspect(|_| *undone_first_run = None),
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> RareDrain<T> {
        match &mut self.0 {
            Inner::NeverRan { ref mut empty, .. } => RareDrain(empty.drain(..0)),
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                if is_undone(undone_first_run) {
                    log.drain_past_by_len(usize::MAX)
                } else {
                    let iter = log.drain_past_by_len(max_past_len);
                    if iter.len() != 0 {
                        *undone_first_run = None;
                    }
                    iter
                }
            }
        }
    }
    fn never_ran_push(&mut self, present: Option<T>, next_skips: usize) {
        match present {
            None => match &mut self.0 {
                Inner::NeverRan { skips, .. } => *skips = next_skips,
                Inner::Ran { .. } => unreachable!(),
            },
            Some(present) => {
                let empty = match take(self).0 {
                    Inner::NeverRan { empty, .. } => empty,
                    Inner::Ran { .. } => unreachable!(),
                };
                self.0 = Inner::Ran {
                    log: RareStateLog::with_alloc(present, empty),
                    undone_first_run: Some(UndoneFirstRun {
                        undone: false,
                        skips_max: next_skips,
                    }),
                };
            }
        }
    }
}

impl<T: LoggedAt> RareInitNoneLog<T> {
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<T> {
        match &mut self.0 {
            Inner::NeverRan { .. } => None,
            Inner::Ran {
                undone_first_run, ..
            } if is_undone(undone_first_run) => None,
            Inner::Ran {
                log,
                undone_first_run,
            } => log
                .pop_past_by_logged_at(meta)
                .inspect(|_| *undone_first_run = None),
        }
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> RareDrain<T> {
        match &mut self.0 {
            Inner::NeverRan { ref mut empty, .. } => RareDrain(empty.drain(..0)),
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                if is_undone(undone_first_run) {
                    log.drain_past_by_len(usize::MAX)
                } else {
                    let iter = log.truncate_future_drain_past_by_logged_at(meta);
                    if iter.len() != 0 {
                        *undone_first_run = None;
                    }
                    iter
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU32;

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
            full: RareInitNoneLog<char>,
            #[serde(with = "crate::log::logless_state")]
            logless: RareInitNoneLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: RareInitNoneLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: RareInitNoneLog<char>,
        }

        impl Logs {
            fn new(log: RareInitNoneLog<char>, reserve_additional: usize) -> Self {
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

        let original_never_ran = RareInitNoneLog::new_none();
        assert!(
            matches!(original_never_ran.0, Inner::NeverRan { .. }),
            "{original_never_ran:?}"
        );

        let mut original_ran_after_none = original_never_ran.clone();
        original_ran_after_none.push_present(Some('a'));
        original_ran_after_none.push_present(Some('b'));
        original_ran_after_none.push_present(Some('c'));
        original_ran_after_none.backward_log().expect("in log");
        assert!(
            matches!(
                original_ran_after_none.0,
                Inner::Ran {
                    undone_first_run: Some(UndoneFirstRun { undone: false, .. }),
                    ..
                }
            ),
            "{original_ran_after_none:#?}"
        );

        let mut original_undone_first_run = original_ran_after_none.clone();
        original_undone_first_run.backward_log().expect("in log");
        original_undone_first_run.backward_log().expect("in log");
        assert!(
            matches!(
                original_undone_first_run.0,
                Inner::Ran {
                    undone_first_run: Some(UndoneFirstRun { undone: true, .. }),
                    ..
                }
            ),
            "{original_undone_first_run:#?}"
        );

        let mut original_never_none = original_ran_after_none.clone();
        if let Inner::Ran {
            undone_first_run, ..
        } = &mut original_never_none.0
        {
            *undone_first_run = None;
        }
        assert!(
            matches!(
                original_never_none.0,
                Inner::Ran {
                    undone_first_run: None,
                    ..
                }
            ),
            "{original_never_none:#?}"
        );

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
            original: RareInitNoneLog<char>,
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

            let test = |log: &RareInitNoneLog<char>, expected_len, with_capacity| {
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

    impl RareInitNoneLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            max_past_len: usize, // control when the by-len strategies trigger pop/drain to align to the by-logged-at strategies
            state: Option<(u8, u32)>,
            state_is_pushed: bool,
            expected_states_len: usize,
            expected_popped: Option<(u8, u32)>,
        ) {
            meta.queue_forward();
            meta.update(|_, _| {});
            let before = self.clone();
            let push = state.filter(|_| state_is_pushed).map(|(state, frame)| (state, RevFrame::checked_new(frame)));
            self.push_present(push);
            let after_push = self.clone();
            let actual_popped =
                shorten_strategy!(self, meta, strategy, max_past_len, before, after_push);
            assert_eq!(
                actual_popped, expected_popped,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            let actual_states_len = self.states_len();
            assert_eq!(
                actual_states_len, expected_states_len,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            self.test_state(before, meta, state);
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: Option<(u8, u32)>,
            expected_result: Result<bool, OutOfLog>,
        ) {
            let before = self.clone();
            let actual_result = self.forward_log();
            if expected_result.is_ok() {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
            }
            assert_eq!(
                actual_result, expected_result,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            self.test_state(before, meta, expected_state);
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: Option<(u8, u32)>,
            expected_result: Result<bool, OutOfLog>,
        ) {
            let before = self.clone();
            let actual_result = self.backward_log();
            if expected_result.is_ok() {
                let frame = meta.present_world_state().wrapping_sub(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
            }
            assert_eq!(
                actual_result, expected_result,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            self.test_state(before, meta, expected_state);
        }
        fn test_state(&self, before: Self, meta: &RevMeta, expected_state: Option<(u8, u32)>) {
            let expected_state =
                expected_state.map(|(state, frame)| (state, RevFrame::checked_new(frame)));
            assert_eq!(
                self.get().cloned(),
                expected_state,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (u8, u32)>,
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
            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut init_none = RareInitNoneLog::new_none();

            init_none.test_forward(meta, strategy, 0, None, false, 0, None);
            init_none.test_forward(meta, strategy, 1, Some((2, 2)), true, 0, None);

            // reaching pre-state None
            init_none.test_backward_log(meta, None, Ok(true));
            init_none.test_backward_log(meta, None, Ok(false));
            // out of log, no mutations happend to both meta and log here
            init_none.test_backward_log(meta, None, Err(OutOfLog));

            init_none.test_forward_log(meta, None, Ok(false));
            // back in state Some
            init_none.test_forward_log(meta, Some((2, 2)), Ok(true));
            // out of log, no mutations happend to both meta and log here
            init_none.test_forward_log(meta, Some((2, 2)), Err(OutOfLog));

            init_none.test_forward(meta, strategy, 2, Some((2, 2)), false, 0, None);

            // has no initial value that is out of log now
            init_none.test_forward(meta, strategy, 3, Some((3, 3)), true, 1, None);

            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut init_some = RareInitNoneLog::new_some((0, meta.present_world_state()));

            init_some.test_forward(meta, strategy, 0, Some((0, 0)), false, 0, None);
            init_some.test_forward(meta, strategy, 1, Some((2, 2)), true, 1, None);

            init_some.test_backward_log(meta, Some((0, 0)), Ok(true));
            init_some.test_backward_log(meta, Some((0, 0)), Ok(false));
            // out of log, no mutations happend to both meta and log here
            init_some.test_backward_log(meta, Some((0, 0)), Err(OutOfLog));

            init_some.test_forward_log(meta, Some((0, 0)), Ok(false));
            init_some.test_forward_log(meta, Some((2, 2)), Ok(true));
            // out of log, no mutations happend to both meta and log here
            init_some.test_forward_log(meta, Some((2, 2)), Err(OutOfLog));

            init_some.test_forward(meta, strategy, 2, Some((2, 2)), false, 1, None);

            // pops the initial value that became out-of-log
            init_some.test_forward(meta, strategy, 3, Some((3, 3)), true, 1, Some((0, 0)));
/* 
todo: forward/backward_init_none überarbeiten

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
            }*/
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareInitNoneLog<RevFrame>>();
    }
}
