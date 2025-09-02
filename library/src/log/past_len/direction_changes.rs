use std::{num::NonZeroU32, sync::atomic::AtomicU32, u64};

use bevy::{
    ecs::{change_detection::MaybeLocation, resource::Resource},
    log::warn,
    utils::Parallel,
};

use crate::meta::RevMeta;

/// todo: mention ResMut unusable in reversible system
#[derive(Resource)]
pub struct PastLenLogs {
    ids: AtomicU32,
    cleared_ids: u64,
    updates: Parallel<Vec<Update>>,
    limits: Vec<Limits>,
    was_log: bool,
    log_exits: u64,
    /// Taken from [`RevMeta`] at each update so [`PastLenLog`] does not need `RevMeta` as a param.
    now: u64,
    /// Taken from [`RevMeta`] at each update so [`PastLenLog`] does not need `RevMeta` as a param.
    past_end: u64,
}

#[derive(Copy, Clone, PartialEq)]
struct Update {
    state: UpdateState,
    limits: Limits,
}

#[derive(Copy, Clone, PartialEq, Debug)]
#[doc(hidden)]
pub struct UpdateState {
    internal_id: u64,
    updates_this_frame: NonZeroU32,
    log_exits: u64,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub(super) struct Limits {
    backward: u64,
    forward: u64,
    last_update: MaybeLocation,
}

impl Limits {
    #[track_caller]
    pub(super) fn not_log_limit(backward: u64) -> Self {
        Self {
            backward,
            forward: u64::MAX,
            last_update: MaybeLocation::caller(),
        }
    }
    #[track_caller]
    pub(super) fn log_limits(backward: u64, forward: u64) -> Self {
        Self {
            backward,
            forward,
            last_update: MaybeLocation::caller(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct PastLenLogsError {
    pub(crate) missed_forward: bool,
    pub(crate) last_update: MaybeLocation,
}

struct UpdatesIter<'a>(Vec<UpdatesLocal<'a>>);

struct UpdatesLocal<'a> {
    drain: std::vec::Drain<'a, Update>,
    next: Update,
}

impl<'a> Iterator for UpdatesIter<'a> {
    type Item = (usize, Limits);
    fn next(&mut self) -> Option<Self::Item> {
        let (index, local) = self
            .0
            .iter_mut()
            .enumerate()
            .min_by_key(|(_, local)| local.next.state.updates_this_frame)?;

        let next = (local.next.state.internal_id as usize, local.next.limits);

        match local.drain.next() {
            Some(update) => {
                local.next = update;
            }
            None => {
                self.0.swap_remove(index);
            }
        }

        Some(next)
    }
}

pub(super) enum StateChange {
    Cleared,
    TruncateFuture,
}

impl PastLenLogs {
    // do not expose a pub constructor
    pub(crate) fn new(meta: &RevMeta) -> Self {
        Self::new_inner(meta.now(), meta.past_end())
    }
    fn new_inner(now: u64, past_end: u64) -> Self {
        Self {
            ids: AtomicU32::new(0),
            cleared_ids: 0,
            updates: Parallel::default(),
            limits: Vec::new(),
            was_log: false,
            log_exits: 0,
            now,
            past_end,
        }
    }
    pub(super) fn now(&self) -> u64 {
        self.now
    }
    pub(super) fn past_end(&self) -> u64 {
        self.past_end
    }
    pub fn clear(&mut self) {
        self.cleared_ids += *self.ids.get_mut() as u64;
        self.ids = AtomicU32::new(0);
        self.updates.clear();
        self.limits.clear();
        self.was_log = false;
        self.log_exits = 0;
    }
    pub(super) fn update_state(
        &self,
        state: &mut Option<UpdateState>,
        last_update: u64,
    ) -> (UpdateState, Option<StateChange>) {
        let new_state = || {
            let id = self.ids.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if id == u32::MAX {
                warn!("todo");
            }
            UpdateState {
                internal_id: id as u64 + self.cleared_ids,
                updates_this_frame: NonZeroU32::MIN,
                log_exits: self.log_exits,
            }
        };
        match state {
            Some(mut state) => {
                if state.internal_id < self.cleared_ids {
                    (new_state(), Some(StateChange::Cleared))
                } else if state.log_exits < self.log_exits {
                    state.log_exits = self.log_exits;
                    (state, Some(StateChange::TruncateFuture))
                } else if last_update == self.now {
                    state.updates_this_frame = state.updates_this_frame.checked_add(1).unwrap();
                    (state, None)
                } else {
                    state.updates_this_frame = NonZeroU32::MIN;
                    (state, None)
                }
            }
            None => (new_state(), None),
        }
    }
    pub(super) fn push(&self, state: UpdateState, limits: Limits) {
        self.updates
            .borrow_local_mut()
            .push(Update { state, limits });
    }
    pub(crate) fn update_before(&mut self, meta: &RevMeta) {
        self.now = meta.now();
        self.past_end = meta.past_end();
    }
    pub(crate) fn update_after(&mut self, meta: &RevMeta) -> Result<(), PastLenLogsError> {
        self.update_from_locals();
        self.check_limits(meta.running_direction().is_log(), meta.now())
    }

    fn update_from_locals(&mut self) {
        // if an error points to this, something went wrong internally
        let placeholder_location = MaybeLocation::caller();

        // size up self.limits if new PastLenLogs pushed one or multiple updates
        self.limits.resize(
            *self.ids.get_mut() as usize,
            Limits {
                backward: u64::MAX, // will cause error if not overwritten
                forward: u64::MIN,  // will cause error if not overwritten
                last_update: placeholder_location,
            },
        );

        let iter = UpdatesIter(
            self.updates
                .iter_mut()
                .flat_map(|vec| {
                    let mut drain = vec.drain(..);
                    drain.next().map(|next| UpdatesLocal { drain, next })
                })
                .collect(),
        );
        for (index, limits) in iter {
            // if a PastLenLog pushed more than one limit, the most recent determines the limits,
            // so if one of the updates in a log frame was missed, this will cause an error
            self.limits[index] = limits;
        }
    }

    fn check_limits(&mut self, log: bool, now: u64) -> Result<(), PastLenLogsError> {
        if log {
            for limits in self.limits.iter() {
                if now < limits.backward {
                    return Err(PastLenLogsError {
                        missed_forward: false,
                        last_update: limits.last_update,
                    });
                }
                if now > limits.forward {
                    return Err(PastLenLogsError {
                        missed_forward: true,
                        last_update: limits.last_update,
                    });
                }
            }

            self.was_log = true;
        } else {
            for limits in self.limits.iter_mut() {
                if now < limits.backward {
                    return Err(PastLenLogsError {
                        missed_forward: false,
                        last_update: limits.last_update,
                    });
                }
                // unset future limits because logs just were or will be truncated
                limits.forward = u64::MAX;
            }

            if self.was_log {
                self.was_log = false;
                self.log_exits = self.log_exits.checked_add(1).unwrap();
            }
        }
        Ok(())
    }
}

#[cfg(testt)]
mod test {
    use super::*;

    #[test]
    fn iter_works() {
        fn new_limits(value: u64) -> Limits {
            Limits {
                backward: value,
                forward: value,
                last_update: MaybeLocation::caller(),
            }
        }

        fn new_state(value: u32) -> UpdateState {
            UpdateState {
                id: value as u64,
                updates_this_frame: NonZeroU32::new(value).unwrap(),
            }
        }

        fn new_update(value: u32) -> Update {
            Update {
                state: new_state(value),
                limits: new_limits(value as u64),
            }
        }

        let mut vec1 = vec![new_update(3), new_update(4), new_update(6)];
        let mut vec2 = vec![new_update(4), new_update(5)];
        let iter = UpdatesIter(vec![
            UpdatesLocal {
                drain: vec1.drain(..),
                next: new_update(1),
            },
            UpdatesLocal {
                drain: vec2.drain(..),
                next: new_update(2),
            },
        ]);

        let actual: Vec<_> = iter.collect();
        let expected = vec![
            (1, new_limits(1)),
            (2, new_limits(2)),
            (3, new_limits(3)),
            (4, new_limits(4)),
            (4, new_limits(4)),
            (5, new_limits(5)),
            (6, new_limits(6)),
        ];

        assert_eq!(actual, expected);
    }

    #[test]
    fn updates_to_results() {
        // arrange
        let last_update = MaybeLocation::caller();
        let mut past_len_logs = PastLenLogs {
            was_log: true,
            log_exits: 1,
            ..PastLenLogs::new_inner(0, 0)
        };

        // add a backward limit of 1
        let mut state = None;
        past_len_logs.push_get_cleared(
            &mut state,
            1,
            Limits {
                backward: 1,
                forward: u64::MAX,
                last_update,
            },
        );
        assert_eq!(
            state,
            Some(UpdateState {
                id: 0,
                updates_this_frame: NonZeroU32::MIN
            })
        );

        // add a forward limit of 1
        let mut state = None;
        past_len_logs.push_get_cleared(
            &mut state,
            1,
            Limits {
                backward: u64::MIN,
                forward: 1,
                last_update,
            },
        );
        assert_eq!(
            state,
            Some(UpdateState {
                id: 1,
                updates_this_frame: NonZeroU32::MIN
            })
        );

        // apply updates
        past_len_logs.update_from_locals();

        // 0 < backward of 1 results in Err
        assert_eq!(
            past_len_logs.check_limits(true, 0),
            Err(PastLenLogsError {
                now: 0,
                missed_forward: false,
                last_update
            })
        );

        // 2 > forward of 1 results in Err
        assert_eq!(
            past_len_logs.check_limits(true, 2),
            Err(PastLenLogsError {
                now: 2,
                missed_forward: true,
                last_update
            })
        );

        // test exited_log_meanwhile
        let mut log_exits = 0;
        assert!(past_len_logs.exited_log_meanwhile(&mut log_exits));
        assert_eq!(log_exits, 1);

        assert!(!past_len_logs.exited_log_meanwhile(&mut log_exits));
        assert_eq!(log_exits, 1);

        // forward not checked, so results in Ok
        assert!(!past_len_logs.exited_log_meanwhile(&mut log_exits));
        assert_eq!(log_exits, 1);
        assert_eq!(past_len_logs.check_limits(false, 2), Ok(()));
        assert!(past_len_logs.exited_log_meanwhile(&mut log_exits));
        assert_eq!(log_exits, 2);

        // forward checked but unset previously, so results in Ok
        assert_eq!(past_len_logs.check_limits(true, 2), Ok(()));
    }
}
