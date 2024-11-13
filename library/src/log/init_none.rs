use std::{
    collections::{TryReserveError, VecDeque},
    mem::take,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{LogIter, LoggedAt, OutOfLog, StateLog};

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
        NeverRan { capacity: usize },
        Ran { log: T, undone_first_run: Option<bool> },
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
                Inner::Ran { ref log, undone_first_run: Some(true) } => (None, log.states_capacity()),
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
        Self(Inner::Ran {
            log: present.into(),
            undone_first_run: None,
        })
    }
}

impl<T> From<Option<T>> for InitNoneLog<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(present) => present.into(),
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
            Inner::Ran { log, .. } => Some(log.into_inner()),
        }
    }
    pub fn get(&self) -> Option<&T> {
        match self.0 {
            Inner::Ran { ref log, undone_first_run } if undone_first_run != Some(true) => {
                Some(&*log)
            },
            _ => None
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
    pub fn drain_future(&mut self) -> impl LogIter<T> {
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
            Inner::Ran { log, .. } => log.drain_future_specific(),
        }
    }
    pub fn clear_log(&mut self) {
        if let Inner::Ran {
            log,
            ..
        } = &mut self.0
        {
            log.clear();
        }
    }
    pub fn clear(&mut self) {
        let empty = match take(self).0 {
            Inner::NeverRan { empty } => empty,
            Inner::Ran { log, .. } => log.into_inner_with_log().1
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
            Inner::Ran { undone_first_run: Some(true), .. } => Err(OutOfLog),
            Inner::Ran { ref mut log, undone_first_run: None } => {
                log.backward_log()
            },
            Inner::Ran { ref mut log, ref mut undone_first_run} => {
                *undone_first_run = Some(log.backward_log() == Err(OutOfLog));
                Ok(())
            },
            Inner::NeverRan { .. } => Err(OutOfLog)
        }
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        match self.0 {
            Inner::Ran { undone_first_run: Some(ref mut undone), .. } if *undone => {
                *undone = false;
                Ok(())
            },
            Inner::Ran { ref mut log, .. } => log.forward_log(),
            Inner::NeverRan { .. } => Err(OutOfLog)
        }
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        match self.0 {
            Inner::Ran { undone_first_run: Some(true), .. } | Inner::NeverRan { .. } => None,
            Inner::Ran {
                ref mut log,
                ref mut undone_first_run
            } => {
                let popped = log.pop_past_by_len(max_past_len);
                if popped.is_some() || log.past_len() == max_past_len {
                    *undone_first_run = None;
                }
                popped
            } 
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
        match self.0 {
            Inner::NeverRan { ref mut empty } => empty.drain(..0),
            Inner::Ran {
                ref mut log,
                undone_first_run: Some(true),
            } => log.drain_past_by_len_specific(usize::MAX),
            Inner::Ran {
                ref mut log, 
                ref mut undone_first_run
            } => {
                let iter = log.drain_past_by_len_specific(max_past_len);
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
            Inner::Ran { undone_first_run: Some(true), .. } | Inner::NeverRan { .. } => None,
            Inner::Ran { 
                ref mut log, 
                ref mut undone_first_run 
            } => {
                let popped = log.pop_past_by_logged_at(meta);
                if popped.is_some() {
                    *undone_first_run = None;
                }
                popped
            }
        }
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> impl LogIter<T> {
        match self.0 {
            Inner::NeverRan { ref mut empty } => empty.drain(..0),
            Inner::Ran {
                ref mut log,
                undone_first_run: Some(true),
            } => log.drain_past_by_len_specific(usize::MAX),
            Inner::Ran {
                ref mut log, 
                ref mut undone_first_run
            } => {
                let iter = log.truncate_future_drain_past_by_logged_at_specific(meta);
                if iter.len() != 0 {
                    *undone_first_run = None;
                }
                iter
            }
        }
    }
}
