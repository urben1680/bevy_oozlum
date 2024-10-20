use bevy::reflect::Reflect;

use super::{OutOfLog, StateLog};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InitiallyNoneStateLog<T>(Inner<StateLog<T>>);

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(super) enum Inner<T> {
    NeverRan { capacity: usize },
    Ran { log: T, undone_first_run: bool },
}

#[cfg(feature = "serde")]
mod serde_with {
    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessState, LoglessWithCapacity, WithCapacity};

    use super::{InitiallyNoneStateLog, Inner, StateLog};

    impl<T> LoglessState for InitiallyNoneStateLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = Option<&'se T>;
        type De = Option<T>;
        fn get_logless_state(&self) -> Self::Se<'_> {
            self.get()
        }
        fn from_logless_state(logless_state: Self::De) -> Result<Self, String> {
            Ok(logless_state.into())
        }
    }

    impl<T> WithCapacity for InitiallyNoneStateLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = Inner<<StateLog<T> as WithCapacity>::Se<'se>>;
        type De = Inner<<StateLog<T> as WithCapacity>::De>;
        fn get_with_capacity(&self) -> Self::Se<'_> {
            match self.0 {
                Inner::NeverRan { capacity } => Inner::NeverRan { capacity },
                Inner::Ran {
                    ref log,
                    undone_first_run,
                } => Inner::Ran {
                    log: log.get_with_capacity(),
                    undone_first_run,
                },
            }
        }
        fn from_with_capacity(with_capacity: Self::De) -> Result<Self, String> {
            Ok(Self(match with_capacity {
                Inner::NeverRan { capacity } => Inner::NeverRan { capacity },
                Inner::Ran {
                    log,
                    undone_first_run,
                } => match StateLog::from_with_capacity(log) {
                    Ok(log) => Inner::Ran {
                        log,
                        undone_first_run,
                    },
                    Err(err) => return Err(err),
                },
            }))
        }
    }

    impl<T> LoglessWithCapacity for InitiallyNoneStateLog<T>
    where
        T: Serialize + for<'de> Deserialize<'de> + 'static,
    {
        type Se<'se> = (Option<&'se T>, usize);
        type De = (Option<T>, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            match self.0 {
                Inner::NeverRan { capacity } => (None, capacity),
                Inner::Ran {
                    ref log,
                    undone_first_run,
                } => ((!undone_first_run).then_some(&*log), log.capacity()),
            }
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Result<Self, String> {
            Ok(Self(match logless_with_capacity.0 {
                Some(present) => Inner::Ran {
                    log: StateLog::with_capacity(present, logless_with_capacity.1),
                    undone_first_run: false,
                },
                None => Inner::NeverRan {
                    capacity: logless_with_capacity.1,
                },
            }))
        }
    }
}

impl<T> Default for InitiallyNoneStateLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> From<T> for InitiallyNoneStateLog<T> {
    fn from(present: T) -> Self {
        Self(Inner::Ran {
            log: present.into(),
            undone_first_run: false,
        })
    }
}

impl<T> From<Option<T>> for InitiallyNoneStateLog<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(present) => present.into(),
            None => Self::new(),
        }
    }
}

impl<T> InitiallyNoneStateLog<T> {
    pub const fn new() -> Self {
        Self::with_capacity(0)
    }
    pub const fn with_capacity(capacity: usize) -> Self {
        Self(Inner::NeverRan { capacity })
    }
    pub fn get(&self) -> Option<&T> {
        if let Inner::Ran {
            log,
            undone_first_run: false,
        } = &self.0
        {
            Some(&*log)
        } else {
            None
        }
    }
    pub fn get_log(&self) -> Option<&StateLog<T>> {
        if let Inner::Ran {
            log,
            undone_first_run: false,
        } = &self.0
        {
            Some(log)
        } else {
            None
        }
    }
    pub fn get_log_unchecked(&self) -> Option<&StateLog<T>> {
        if let Inner::Ran { log, .. } = &self.0 {
            Some(log)
        } else {
            None
        }
    }
    pub fn get_log_mut(&mut self) -> Option<&mut StateLog<T>> {
        if let Inner::Ran {
            log,
            undone_first_run: false,
        } = &mut self.0
        {
            Some(log)
        } else {
            None
        }
    }
    pub fn get_log_mut_unchecked(&mut self) -> Option<&mut StateLog<T>> {
        if let Inner::Ran { log, .. } = &mut self.0 {
            Some(log)
        } else {
            None
        }
    }
    pub fn push_present(&mut self, state: T) -> &mut StateLog<T> {
        match self.0 {
            Inner::Ran {
                ref mut log,
                ref mut undone_first_run,
            } => {
                if *undone_first_run {
                    log.clear_with(state);
                    *undone_first_run = false;
                } else {
                    log.push_present(state)
                }
                log
            }
            Inner::NeverRan { capacity } => {
                self.0 = Inner::Ran {
                    log: StateLog::with_capacity(state, capacity),
                    undone_first_run: false,
                };
                match &mut self.0 {
                    Inner::Ran { log, .. } => log,
                    Inner::NeverRan { .. } => unreachable!(),
                }
            }
        }
    }
    // todo: may incorrectly return `Ok` if `clear` was called before
    pub fn forward_log(&mut self) -> Result<&mut StateLog<T>, OutOfLog> {
        match &mut self.0 {
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                if *undone_first_run {
                    *undone_first_run = false;
                    Ok(log)
                } else {
                    log.forward_log().map(|()| log)
                }
            }
            Inner::NeverRan { .. } => Err(OutOfLog),
        }
    }
    // todo: may incorrectly return `Ok` if constructed from T
    pub fn backward_log(&mut self) -> Result<&mut StateLog<T>, OutOfLog> {
        match &mut self.0 {
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                if *undone_first_run {
                    Err(OutOfLog)
                } else {
                    *undone_first_run = log.backward_log() == Err(OutOfLog);
                    Ok(log)
                }
            }
            Inner::NeverRan { .. } => Err(OutOfLog),
        }
    }
    pub fn clear(&mut self) {
        if let Inner::Ran {
            log,
            undone_first_run,
        } = &mut self.0
        {
            log.clear();
            *undone_first_run = true;
        }
    }
    pub fn clear_with(&mut self, present: T) {
        match &mut self.0 {
            Inner::Ran {
                log,
                undone_first_run,
            } => {
                log.clear_with(present);
                *undone_first_run = true;
            }
            Inner::NeverRan { capacity } => {
                self.0 = Inner::Ran {
                    log: StateLog::with_capacity(present, *capacity),
                    undone_first_run: true,
                }
            }
        }
    }
}
