use bevy::reflect::Reflect;

use super::{OutOfLog, StateLog};

#[derive(Debug, Clone, Reflect)]
pub struct InitiallyNoneStateLog<T>(Sealed<T>);

#[derive(Debug, Clone, Reflect)]
enum Sealed<T> {
    NeverRan {
        capacity: usize,
    },
    Ran {
        log: StateLog<T>,
        undone_first_run: bool,
    },
}

impl<T> Default for InitiallyNoneStateLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> From<T> for InitiallyNoneStateLog<T> {
    fn from(present: T) -> Self {
        Self(Sealed::Ran {
            log: present.into(),
            undone_first_run: false,
        })
    }
}

impl<T> InitiallyNoneStateLog<T> {
    pub const fn new() -> Self {
        Self::with_capacity(0)
    }
    pub const fn with_capacity(capacity: usize) -> Self {
        Self(Sealed::NeverRan { capacity })
    }
    pub fn get(&self) -> Option<&T> {
        if let Sealed::Ran {
            log,
            undone_first_run: false,
        } = &self.0
        {
            Some(&*log)
        } else {
            None
        }
    }
    pub fn unlogged_get_mut(&mut self) -> Option<&mut T> {
        if let Sealed::Ran {
            log,
            undone_first_run: false,
        } = &mut self.0
        {
            Some(log.unlogged_get_mut())
        } else {
            None
        }
    }
    pub fn get_log(&self) -> Option<&StateLog<T>> {
        if let Sealed::Ran {
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
        if let Sealed::Ran { log, .. } = &self.0 {
            Some(log)
        } else {
            None
        }
    }
    pub fn get_log_mut(&mut self) -> Option<&mut StateLog<T>> {
        if let Sealed::Ran {
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
        if let Sealed::Ran { log, .. } = &mut self.0 {
            Some(log)
        } else {
            None
        }
    }
    pub fn push_present(&mut self, state: T) -> &mut StateLog<T> {
        match self.0 {
            Sealed::Ran {
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
            Sealed::NeverRan { capacity } => {
                self.0 = Sealed::Ran {
                    log: StateLog::with_capacity(state, capacity),
                    undone_first_run: false,
                };
                match &mut self.0 {
                    Sealed::Ran { log, .. } => log,
                    Sealed::NeverRan { .. } => unreachable!(),
                }
            }
        }
    }
    pub fn forward_log(&mut self) -> Result<&mut StateLog<T>, OutOfLog> {
        match &mut self.0 {
            Sealed::Ran {
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
            Sealed::NeverRan { .. } => Err(OutOfLog),
        }
    }
    pub fn backward_log(&mut self) -> Result<&mut StateLog<T>, OutOfLog> {
        match &mut self.0 {
            Sealed::Ran {
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
            Sealed::NeverRan { .. } => Err(OutOfLog),
        }
    }
    pub fn clear(&mut self) {
        if let Sealed::Ran {
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
            Sealed::Ran {
                log,
                undone_first_run,
            } => {
                log.clear_with(present);
                *undone_first_run = true;
            }
            Sealed::NeverRan { capacity } => {
                self.0 = Sealed::Ran {
                    log: StateLog::with_capacity(present, *capacity),
                    undone_first_run: true,
                }
            }
        }
    }
}
