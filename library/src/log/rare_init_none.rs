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
