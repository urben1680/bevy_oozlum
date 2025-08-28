use std::{num::NonZeroU64, ops::Deref, sync::atomic::AtomicU32};

use bevy::{
    ecs::{change_detection::MaybeLocation, component::{ComponentId, Tick}, query::FilteredAccessSet, resource::Resource, system::{Res, SystemChangeTick, SystemMeta, SystemParam, SystemParamValidationError}, world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, World}},
    log::warn,
    utils::Parallel,
};

use crate::meta::RevMeta;

#[derive(Resource)]
pub struct PastLenLogs {
    ids: AtomicU32,
    updates: Parallel<Vec<Update>>,
    limits: Vec<Limits>,
    was_log: bool,
    log_exits: NonZeroU64
}

#[derive(Copy, Clone, PartialEq)]
struct Update {
    tick: Tick,
    id: u32,
    limits: Limits,
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub(super) struct Limits {
    backward: u64,
    forward: u64,
    last_update: MaybeLocation,
}

#[derive(Debug, PartialEq)]
pub(crate) struct PastLenLogsError {
    now: u64,
    missed_forward: bool,
    last_update: MaybeLocation,
}

struct UpdatesIter<'a> {
    updates_locals: Vec<UpdatesLocal<'a>>,
    this_run: Tick,
}

struct UpdatesLocal<'a> {
    drain: std::vec::Drain<'a, Update>,
    next: Update,
}

impl<'a> Iterator for UpdatesIter<'a> {
    type Item = (usize, Limits);
    fn next(&mut self) -> Option<Self::Item> {
        let index = self
            .updates_locals
            .iter()
            .enumerate()
            .min_by(|(_, local1), (_, local2)| {
                use std::cmp::Ordering;
                if local1
                    .next
                    .tick
                    .is_newer_than(local2.next.tick, self.this_run)
                {
                    Ordering::Greater
                } else if local1.next.tick == local2.next.tick {
                    Ordering::Equal
                } else {
                    Ordering::Less
                }
            })?
            .0;

        let local = &mut self.updates_locals[index];
        let next = (local.next.id as usize, local.next.limits);
        match local.drain.next() {
            Some(update) => {
                local.next = update;
            }
            None => {
                self.updates_locals.swap_remove(index);
            }
        }

        Some(next)
    }
}

impl PastLenLogs {
    // do not expose a pub constructor
    pub(crate) fn new() -> Self {
        Self { 
            ids: AtomicU32::new(0), 
            updates: Parallel::default(), 
            limits: Vec::new(),
            was_log: false,
            log_exits: NonZeroU64::MIN
        }
    }
    pub(super) fn exited_log(&self, log_exits: &mut Option<NonZeroU64>) -> bool {
        match log_exits {
            Some(log_exits) if log_exits.get() < self.log_exits.get() => {
                *log_exits = self.log_exits;
                true
            },
            Some(_) => false,
            None => {
                *log_exits = Some(self.log_exits);
                false
            }
        }
    }
    pub(super) fn push(&self, id: &mut Option<u32>, tick: Tick, limits: Limits) {
        let id = *id.get_or_insert_with(|| {
            let id = self.ids.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if id == u32::MAX {
                warn!("todo");
            }
            id
        });
        self.updates
            .borrow_local_mut()
            .push(Update { tick, id, limits });
    }
    pub(crate) fn update(
        &mut self,
        meta: &RevMeta,
        this_run: Tick,
    ) -> Result<(), PastLenLogsError> {
        self.update_from_locals(this_run);
        self.check_limits(meta.running_direction().is_log(), meta.now())
    }

    fn update_from_locals(&mut self, this_run: Tick) {
        // if an error points to this, something went wrong
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

        let iter = UpdatesIter {
            updates_locals: self
                .updates
                .iter_mut()
                .flat_map(|vec| {
                    let mut drain = vec.drain(..);
                    drain.next().map(|next| UpdatesLocal { drain, next })
                })
                .collect(),
            this_run,
        };
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
                        now,
                        missed_forward: false,
                        last_update: limits.last_update,
                    });
                }
                if now > limits.forward {
                    return Err(PastLenLogsError {
                        now,
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
                        now,
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

// todo: getter in RevWorld
/// A [`SystemParam`] combining immutable access to [`RevMeta`], [`PastLenLogs`] and the present
/// [`Tick`] needed for the [`PastLenLog`] methods.
pub struct RevMetaPastLenLogs<'w> {
    pub meta: &'w RevMeta,
    pub past_len_logs: &'w PastLenLogs,
    pub(crate) this_run: Tick
}

type ResTuple<'w> = (
    Res<'w, RevMeta>,
    Res<'w, PastLenLogs>
);

// SAFETY: uses first-party derive of above struct and just maps it
unsafe impl<'w> SystemParam for RevMetaPastLenLogs<'w> {
    type Item<'world, 'state> = RevMetaPastLenLogs<'world>;
    type State = <ResTuple<'w> as SystemParam>::State;
    
    fn init_state(world: &mut World) -> Self::State {
        <ResTuple<'w> as SystemParam>::init_state(world)
    }

    fn init_access(
        state: &Self::State,
        system_meta: &mut SystemMeta,
        component_access_set: &mut FilteredAccessSet<ComponentId>,
        world: &mut World,
    ) {
        <ResTuple<'w> as SystemParam>::init_access(
            state, 
            system_meta, 
            component_access_set, 
            world
        );
    }

    fn apply(state: &mut Self::State, system_meta: &SystemMeta, world: &mut World) {
        <ResTuple<'w> as SystemParam>::apply(state, system_meta, world);
    }

    fn queue(state: &mut Self::State, system_meta: &SystemMeta, world: DeferredWorld) {
        <ResTuple<'w> as SystemParam>::queue(state, system_meta, world);
    }

    unsafe fn validate_param(
        state: &mut Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // SAFETY: RevMetaPastLenLogs uses same safety contract as ResTuple
        unsafe {
            <ResTuple<'w> as SystemParam>::validate_param(state, system_meta, world)
        }
    }

    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell<'world>,
        change_tick: Tick,
    ) -> Self::Item<'world, 'state> {
        // SAFETY: RevMetaPastLenLogs uses same safety contract as ResTuple
        let item = unsafe {
            <ResTuple<'w> as SystemParam>::get_param(state, system_meta, world, change_tick)
        };
        RevMetaPastLenLogs {
            meta: item.0.into_inner(),
            past_len_logs: item.1.into_inner(),
            this_run: change_tick
        }
    }
}

#[cfg(test)]
mod test {
    use std::u64;

    use super::*;

    #[test]
    fn iter_works() {
        fn new_limits(value: u64) -> Limits {
            Limits {
                backward: value, 
                forward: value, 
                last_update: MaybeLocation::caller() 
            }
        }
    
        fn new_update(value: u32) -> Update {
            Update {
                tick: Tick::new(value),
                id: value,
                limits: new_limits(value as u64)
            }
        }

        let mut vec1 = vec![
            new_update(3),
            new_update(4),
            new_update(6)
        ];
        let mut vec2 = vec![
            new_update(4),
            new_update(5)
        ];
        let iter = UpdatesIter {
            updates_locals: vec![
                UpdatesLocal { 
                    drain: vec1.drain(..), 
                    next: new_update(1)
                },
                UpdatesLocal { 
                    drain: vec2.drain(..), 
                    next: new_update(2)
                },
            ],
            this_run: Tick::new(7),
        };

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
        let two_log_exits = NonZeroU64::new(2).unwrap();
        let mut past_len_logs = PastLenLogs {
            was_log: true,
            log_exits: two_log_exits,
            ..PastLenLogs::new()
        };

        // add a backward limit of 1
        let mut id = None;
        past_len_logs.push(&mut id, Tick::new(0), Limits {
            backward: 1,
            forward: u64::MAX,
            last_update
        });
        assert_eq!(id, Some(0));

        // add a forward limit of 1
        let mut id = None;
        past_len_logs.push(&mut id, Tick::new(0), Limits {
            backward: u64::MIN,
            forward: 1,
            last_update
        });
        assert_eq!(id, Some(1));

        // apply updates
        past_len_logs.update_from_locals(Tick::new(1));

        // test exited_log
        let mut log_exits = None;
        assert!(!past_len_logs.exited_log(&mut log_exits));
        assert_eq!(log_exits, Some(two_log_exits));

        log_exits = Some(NonZeroU64::MIN);
        assert!(past_len_logs.exited_log(&mut log_exits));
        assert_eq!(log_exits, Some(two_log_exits));

        log_exits = Some(two_log_exits);
        assert!(!past_len_logs.exited_log(&mut log_exits));
        assert_eq!(log_exits, Some(two_log_exits));

        // 0 < backward of 1 results in Err
        assert_eq!(past_len_logs.check_limits(true, 0), Err(PastLenLogsError {
            now: 0,
            missed_forward: false,
            last_update
        }));

        // 2 > forward of 1 results in Err
        assert_eq!(past_len_logs.check_limits(true, 2), Err(PastLenLogsError {
            now: 2,
            missed_forward: true,
            last_update
        }));

        // forward not checked, so results in Ok
        let mut log_exits = Some(two_log_exits);
        assert!(!past_len_logs.exited_log(&mut log_exits));
        assert_eq!(log_exits, Some(two_log_exits));
        assert_eq!(past_len_logs.check_limits(false, 2), Ok(()));
        assert!(past_len_logs.exited_log(&mut log_exits));
        assert_eq!(log_exits, Some(NonZeroU64::new(3).unwrap()));

        // forward checked but unset previously, so results in Ok
        assert_eq!(past_len_logs.check_limits(true, 2), Ok(()));
    }
}
