use core::num::NonZeroU64;
use std::{error::Error, fmt::{Debug, Display}, num::NonZeroU32, sync::atomic::AtomicU32};

use bevy::{
    ecs::{
        change_detection::{MaybeLocation, Mut}, component::{ComponentId, Tick}, error::BevyError, resource::Resource, schedule::Schedules, system::{ReadOnlySystemParam, Res, SystemMeta, SystemParam, SystemParamValidationError}, world::{unsafe_world_cell::UnsafeWorldCell, World}
    },
    log::{info, warn},
    reflect::{std_traits::ReflectDefault, Reflect}, utils::Parallel,
};

use crate::{prelude::RevUpdate, undo_redo::{BundleIdOfOpCache, RevDespawnCleaner, UndoRedoBuffer}};

/*
task:
- combine RevMeta and PastLenLogs
- do not allow clone
- simplify log traversal: no jump to specific frames, just forward, forward log, backward log, pause
- Parallel does not impl Debug, Reflect, serde
  - discourage mit-update serialization
  - disable such fields in other logs for serialization?
  - no sub-log values except PastLenState::updates_this_frame which is overwritten anyway
- disable log serde/reflect again, the system state contains undoredo logs and these cannot be seen
  by the user to serde them
- default for direction: pause or not log?
- nothing in RevMeta is needed for a current-world snapshot, so no deserialize
*/

#[derive(Reflect, Resource)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
struct RevMeta {
    #[reflect(skip_serializing)]
    past_end: u64,
    now: u64,
    future_end: u64,
    max_world_states: Option<NonZeroU64>,
    direction: Option<RunningOrRan>,
    queue: Option<Queue>,
    log_exits: u64,
    past_len_ids: AtomicU32,
    past_len_ids_cleared: u64,
    #[cfg_attr(
        feature = "serialize",
        serde(skip)
    )]
    #[reflect(ignore)]
    past_len_updates: Parallel<Vec<PastLenUpdate>>,
    past_len_limits: Vec<PastLenLimits>,
}

impl Debug for RevMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RevMeta")
            .field("past_end", &self.past_end)
            .field("now", &self.now)
            .field("future_end", &self.future_end)
            .field("max_world_states", &self.max_world_states)
            .field("direction", &self.direction)
            .field("queue", &self.queue)
            .field("log_exits", &self.log_exits)
            .field("past_len_ids", &self.past_len_ids)
            .field("past_len_ids_cleared", &self.past_len_ids_cleared)
            .field("past_len_limits", &self.past_len_limits)
            .finish_non_exhaustive()
    }
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Some(NonZeroU64::MIN), false)
    }
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
enum Direction {
    Forward { log: bool },
    BackwardLog,
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
enum RunningOrRan {
    Running(Direction),
    Ran(Direction),
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
enum Queue {
    Run(Direction),
    Pause,
    ClearThenRun(Direction),
    ClearThenPause
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
struct PastLenUpdate {
    state: PastLenState,
    limits: PastLenLimits,
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
struct PastLenState {
    id: u64,
    updates_this_frame: NonZeroU32,
    log_exits: u64,
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
struct PastLenLimits {
    backward: u64,
    forward: u64,
    #[cfg_attr(
        feature = "serialize",
        serde(
            skip_serializing,
            deserialize_with = "maybe_location_deserialize"
        )
    )]
    last_update: MaybeLocation,
}

#[cfg(feature = "serialize")]
fn maybe_location_deserialize<'de, D: serde::Deserializer<'de>>(
    _: D
) -> Result<MaybeLocation, D::Error> {
    // location information cannot be deserialized
    Ok(MaybeLocation::caller())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonLogNow(pub(crate) u64);

impl NonLogNow {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl RevMeta {
    pub fn new(max_world_states: Option<NonZeroU64>, paused: bool) -> Self {
        Self {
            past_end: 0,
            now: 0,
            future_end: 0,
            max_world_states,
            direction: (!paused).then_some(RunningOrRan::Ran(Direction::NOT_LOG)),
            queue: None,
            log_exits: 0,
            past_len_ids: AtomicU32::new(0),
            past_len_ids_cleared: 0,
            past_len_updates: Parallel::default(),
            past_len_limits: Vec::new(),
        }
    }
    pub fn running_direction(&self) -> Direction {
        self.get_running_direction().unwrap()
    }
    pub fn get_running_direction(&self) -> Option<Direction> {
        match self.direction {
            Some(RunningOrRan::Running(direction)) => Some(direction),
            _ => None
        }
    }
    pub fn get_ran_direction(&self) -> Option<Direction> {
        match self.direction {
            Some(RunningOrRan::Ran(direction)) => Some(direction),
            _ => None
        }
    }
    pub fn paused(&self) -> bool {
        self.direction.is_none()
    }
    pub fn future_end(&self) -> u64 {
        self.future_end
    }
    pub const fn now(&self) -> u64 {
        self.now
    }
    pub fn non_log_now(&self) -> Option<NonLogNow> {
        matches!(self.direction, Some(RunningOrRan::Running(Direction::NOT_LOG)))
            .then_some(NonLogNow(self.now))
    }
    pub fn past_end(&self) -> u64 {
        self.past_end
    }
    pub fn past_len(&self) -> u64 {
        self.now - self.past_end
    }
    pub fn future_len(&self) -> u64 {
        self.future_end - self.now
    }
    pub fn len(&self) -> u64 {
        self.future_end - self.past_end + 1 // both ends are inclusive
    }
    pub fn contains(&self, frame: u64) -> bool {
        self.future_end.wrapping_sub(frame) <= (self.future_end - self.past_end)
    }
    pub fn past_contains(&self, frame: u64) -> bool {
        self.now.wrapping_sub(frame).wrapping_sub(1) < (self.now - self.past_end)
    }
    pub fn future_contains(&self, frame: u64) -> bool {
        self.future_end.wrapping_sub(frame) < (self.future_end - self.now)
    }
    pub fn clear(&mut self) {
        assert!(!matches!(self.direction, Some(RunningOrRan::Running(_))));
        self.past_end = 0;
        self.now = 0;
        self.future_end = 0;
        self.past_len_ids_cleared += *self.past_len_ids.get_mut() as u64;
        self.past_len_ids = AtomicU32::new(0);
        self.past_len_updates.clear();
        self.past_len_limits.clear();
        self.log_exits = 0;
    }
    pub fn queue(&mut self, direction: Direction) {
        self.queue = Some(Queue::Run(direction));
    }
    pub fn queue_pause(&mut self) {
        self.queue = Some(Queue::Pause);
    }
    pub fn queue_clear_then(&mut self, direction: Direction) {
        self.queue = Some(Queue::ClearThenRun(direction));
    }
    pub fn queue_clear_then_pause(&mut self) {
        self.queue = Some(Queue::ClearThenPause);
    }
    pub fn try_run_rev_update(world: &mut World) -> Result<(), TryRunRevUpdateError> {
        fn info_missing_meta_or_schedule<const MISSING_META: bool>(world: &mut World) {
            /// Use a Resource instead of Local so this system can be added multiple times and keeps track globally
            #[derive(Resource, Clone, Copy)]
            struct Existed<const MISSING_META: bool>(bool);

            let existed = world.remove_resource::<Existed::<MISSING_META>>();
            world.insert_resource(Existed::<MISSING_META>(false));
            match existed {
                None if MISSING_META => info!(
                    "resource RevMeta does not exist yet, schedule RevUpdate will not be run until it is inserted"
                ),
                None => info!(
                    "schedule RevUpdate does not exist yet, it will not be run until it is inserted"
                ),
                Some(Existed(true)) if MISSING_META => info!(
                    "resource RevMeta was removed, reversible schedule RevUpdate will not be run until it is inserted again"
                ),
                Some(Existed(true)) => info!(
                    "schedule RevUpdate was removed, it will not be run until it is inserted again"
                ),
                Some(Existed(false)) => {}
            };
        }

        world.try_schedule_scope(RevUpdate, |world, schedule| {
            let Some(mut meta) = world.remove_resource::<Self>() else {
                info_missing_meta_or_schedule::<true>(world);
                // RevMeta missing is not an error but a valid way to make RevUpdate not run
                return Ok(());
            };

            world.init_resource::<RevDespawnCleaner>();
            world.init_resource::<BundleIdOfOpCache>();

            let update_result = meta.update(|meta, direction| {
                world.insert_resource(meta);
                schedule.run(world);
                world.remove_resource::<Self>()
            });
            
            fn rev_despawn_cleaner_result(
                world: &mut World, meta: RevMeta, past_len_logs_missed: Vec<PastLenLogMissed>
            ) -> Result<RevMeta, TryRunRevUpdateError> {
                let ok_if_present = world.try_resource_scope::<RevDespawnCleaner, _>(|world, mut rev_despawn_cleaner| {
                    rev_despawn_cleaner.update(&meta, world).is_ok()
                });
                match ok_if_present {
                    Some(true) if past_len_logs_missed.is_empty() => Ok(meta),
                    Some(true) => Err(TryRunRevUpdateError::AfterRunErrors { 
                        meta, 
                        past_len_logs_missed, 
                        rev_despawn_cleaner_out_of_log: false, 
                        rev_despawn_cleaner_removed: false 
                    }),
                    Some(false) => Err(TryRunRevUpdateError::AfterRunErrors { 
                        meta,
                        past_len_logs_missed,
                        rev_despawn_cleaner_out_of_log: true,
                        rev_despawn_cleaner_removed: false 
                    }),
                    None => Err(TryRunRevUpdateError::AfterRunErrors { 
                        meta,
                        past_len_logs_missed,
                        rev_despawn_cleaner_out_of_log: false,
                        rev_despawn_cleaner_removed: true 
                    })
                }
            }

            match update_result {
                Ok(meta) => {
                    let Some(mut rev_despawn_cleaner) = world.get_resource_mut::<RevDespawnCleaner>() {

                    }

                    let mut rev_despawn_cleaner_out_of_log = false;
                    let mut rev_despawn_cleaner_removed = true;
                    if let Some(mut rev_despawn_cleaner) = world.get_resource_mut::<RevDespawnCleaner>() {
                        rev_despawn_cleaner_out_of_log = rev_despawn_cleaner.update(meta, world).is_err();
                    }

                    todo!()
                },
                Err(RevMetaUpdateErr::AlreadyRunning { meta, direction }) => Err(
                    TryRunRevUpdateError::AlreadyRunning { meta, direction }
                ),
                Err(RevMetaUpdateErr::RevMetaNotReturned) => Err(
                    TryRunRevUpdateError::AfterRunErrors { 
                        meta_if_not_removed: None, 
                        past_len_logs_missed: Vec::new(), 
                        rev_despawn_cleaner_out_of_log: todo!(), 
                        rev_despawn_cleaner_removed: todo!()
                    }
                )
            }
        }).unwrap_or_else(|_| {
            info_missing_meta_or_schedule::<false>(world);
            // RevUpdate missing is not an error but a valid way to make it not run
            return Ok(());
        })



/* 
        // replace method with manual check as meta cannot be cloned
        world.try_resource_scope(|world: &mut World, mut meta: Mut<Self>| {
            world.init_resource::<RevDespawnCleaner>();
            world.init_resource::<BundleIdOfOpCache>();

            let buffer = world.get_resource_or_init::<UndoRedoBuffer>();
            if !buffer.is_empty() {
                Err(TryRunRevUpdateError::UndoRedoBufferNotEmptyBeforeUpdate {
                    meta: meta.clone(),
                    buffer_types: format!("{buffer:?}"),
                })?;
            }

            if meta.get_running_direction().is_some() {
                Err(TryRunRevUpdateError::UnexpectedInitialRunning(meta.clone()))?;
            }

            // revert to previous state in case the schedule could not be run
            let previous = meta.clone();

            // update meta here
            let result = meta.update(|meta| {
                // run schedule
                let schedule_result = world.try_schedule_scope(RevUpdate, |world, schedule| {
                    world.insert_resource(meta.clone());
                    schedule.run(world);
                });

                match schedule_result {
                    // despawn entities that are marked as reversibly despawned for long enough
                    // despawn buffer entities for operations that are out of log now
                    Ok(()) => world
                        .try_resource_scope(|world: &mut World, mut res: Mut<RevDespawnCleaner>| {
                            res
                                .update(meta, world)
                                .map(|()| meta.now())
                                .map_err(|OutOfLog| TryRunRevUpdateError::SpawnDespawnOutOfLog(meta.clone()))
                        })
                        .unwrap_or_else(|| {
                            Err(TryRunRevUpdateError::SpawnDespawnRemovedInSchedule(meta.clone()))
                        }),
                    Err(_) => Err(TryRunRevUpdateError::RevUpdateMissing(meta.clone())),
                }
            });

            match result.transpose() {
                Ok(None) => Ok(()),
                // remove meta as a guard against invalid insertions and to make resource_scope not panic
                Ok(Some(frame)) => match world.remove_resource::<Self>() {
                    None => {
                        Err(TryRunRevUpdateError::RevMetaRemovedInSchedule { frame })?
                    }
                    Some(updated) => {
                        // updates to these fields are valid changes to meta, keep them
                        meta.max_world_states = updated.max_world_states;
                        meta.queue = updated.queue;
                        Ok(())
                    }
                },
                Err(err) => {
                    world.remove_resource::<Self>();
                    *meta = previous;
                    Err(err)?
                }
            }
        }).unwrap_or_else(|| {
            /// Use a Resource instead of Local so this system can be added multiple times and keeps track globally
            #[derive(Resource, Clone, Copy)]
            struct Existed(bool);

            let existed = world.remove_resource::<Existed>();
            world.insert_resource(Existed(false));
            match existed {
                None => info!(
                    "RevMeta does not exist yet, reversible schedule RevUpdate will not be called until it is inserted"
                ),
                Some(Existed(true)) => info!(
                    "RevMeta was removed, reversible schedule RevUpdate will not be called until it is inserted again"
                ),
                Some(Existed(false)) => {}
            };

            // `RevMeta` missing is not an error but a valid way to make `RevUpdate` not run
            return Ok(());
        })
        */
    }

    pub fn update(mut self, c: impl FnOnce(Self, Direction) -> Option<Self>)
     -> Result<Self, RevMetaUpdateErr> {
        let mut was_log = false;

        // get direction that ran previously
        let ran = match self.direction {
            Some(RunningOrRan::Ran(direction)) => {
                was_log = direction.is_log();
                Some(direction)
            },
            Some(RunningOrRan::Running(direction)) => return Err(
                RevMetaUpdateErr::AlreadyRunning { meta: self, direction }
            ),
            None => None
        };

        // get queued direction
        let queue = match self.queue.take() {
            Some(Queue::Run(direction)) => Some(direction),
            Some(Queue::Pause) => None,
            Some(Queue::ClearThenRun(direction)) => {
                self.clear();
                Some(direction)
            },
            Some(Queue::ClearThenPause) => {
                self.clear();
                None
            },
            None => None
        };

        // take queue or fall back to previous direction, return None if no direction from both
        let Some(queue_or_ran) = ran.or(queue) else {
            return Ok(self);
        };
        
        // update frame fields or return None if pause is queued or reached end of log
        let direction = match queue_or_ran {
            Direction::NOT_LOG => {
                self.now += 1;
                self.future_end = self.now;
                if was_log {
                    self.log_exits = self.log_exits.checked_add(1).unwrap();
                }
                if let Some(max_world_states) = self.max_world_states.map(NonZeroU64::get) {
                    // include equality here as the present state has to be added to the comparision
                    if self.past_len() >= max_world_states {
                        self.past_end = self.now + 1 - max_world_states;
                    }
                }
                Direction::NOT_LOG
            },
            Direction::FORWARD_LOG if self.now < self.future_end => {
                self.now = self.now + 1;
                Direction::FORWARD_LOG
            },
            Direction::BackwardLog if self.now > self.past_end => {
                self.now = self.now - 1;
                Direction::BackwardLog
            },
            _ => {
                self.direction = None;
                return Ok(self);
            }
        };

        // set running direction, call closure, set ran direction
        self.direction = Some(RunningOrRan::Running(direction));
        let Some(mut meta) = c(self, direction) else {
            return Err(RevMetaUpdateErr::RevMetaNotReturned);
        };
        meta.direction = Some(RunningOrRan::Ran(direction));

        // size up self.past_len_limits if new PastLenLogs updated in the closure
        meta.past_len_limits.resize(
            *meta.past_len_ids.get_mut() as usize,
            PastLenLimits {
                // will cause error if both are not overwritten
                backward: u64::MAX,
                forward: u64::MIN,

                // if an error points to this, something went wrong internally
                last_update: MaybeLocation::caller(),
            },
        );

        // update limits of PastLenLog instances that updated in the closure
        let iter = UpdatesIter(
            meta.past_len_updates
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
            meta.past_len_limits[index] = limits;
        }

        // check limits of all PastLenLog instances
        let iter = meta.past_len_limits.iter_mut().enumerate();
        let mut past_len_logs_missed = Vec::new();
        if direction.is_log() {
            for (id, limits) in iter {
                let internal_id = id as u64;

                if meta.now < limits.backward {
                    past_len_logs_missed.push(PastLenLogMissed {
                        internal_id,
                        missed_forward: false,
                        last_update: limits.last_update,
                    });
                } else if meta.now > limits.forward {
                    past_len_logs_missed.push(PastLenLogMissed {
                        internal_id,
                        missed_forward: true,
                        last_update: limits.last_update,
                    });
                }
            }
        } else {
            for (id, limits) in iter {
                let internal_id = id as u64;

                // unset future limits because logs just were or will be truncated
                limits.forward = u64::MAX;

                if meta.now < limits.backward {
                    past_len_logs_missed.push(PastLenLogMissed {
                        internal_id,
                        missed_forward: false,
                        last_update: limits.last_update,
                    });
                }
            }
        }

        if past_len_logs_missed.is_empty() {
            Ok(meta)
        } else {
            Err(RevMetaUpdateErr::PastLenLogsMissed { meta, past_len_logs_missed })
        }
    }
    pub(super) fn update_past_len_state(
        &self,
        state: Option<PastLenState>,
        last_update: u64,
    ) -> (PastLenState, Option<StateChange>) {
        let new_state = || {
            let id = self.past_len_ids.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if id == u32::MAX {
                warn!("todo");
            }
            PastLenState {
                id: id as u64 + self.past_len_ids_cleared,
                updates_this_frame: NonZeroU32::MIN,
                log_exits: self.log_exits,
            }
        };
        match state {
            Some(mut state) => {
                if state.id < self.past_len_ids_cleared {
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
    pub(super) fn push_past_len_update(&self, state: PastLenState, limits: PastLenLimits) {
        self.past_len_updates
            .borrow_local_mut()
            .push(PastLenUpdate { state, limits });
    }
}

pub enum RevMetaUpdateErr {
    AlreadyRunning {
        meta: RevMeta,
        direction: Direction
    },
    RevMetaNotReturned,
    PastLenLogsMissed {
        meta: RevMeta,
        past_len_logs_missed: Vec<PastLenLogMissed>
    }
}

pub struct PastLenLogMissed {
    internal_id: u64,
    missed_forward: bool,
    last_update: MaybeLocation,
}
/*
errors:

- UndoRedoBufferNotEmptyBeforeUpdate
- UnexpectedInitialRunning
- RevUpdateMissing
- multiple at once:
  - PastLenLogsMissed
  - SpawnDespawnOutOfLog or SpawnDespawnRemovedInSchedule
  - RevMetaRemovedInSchedule
 */

pub enum TryRunRevUpdateError {
    UndoRedoBufferNotEmptyBeforeUpdate {
        meta: RevMeta,
        buffer_types: String,
    },
    AlreadyRunning {
        meta: RevMeta,
        direction: Direction,
    },
    RevMetaRemovedAfterRun,
    AfterRunErrors {
        meta: RevMeta,
        past_len_logs_missed: Vec<PastLenLogMissed>,
        rev_despawn_cleaner_out_of_log: bool,
        rev_despawn_cleaner_removed: bool
    }
}

struct UpdatesIter<'a>(Vec<UpdatesLocal<'a>>);

struct UpdatesLocal<'a> {
    drain: std::vec::Drain<'a, PastLenUpdate>,
    next: PastLenUpdate,
}

impl<'a> Iterator for UpdatesIter<'a> {
    type Item = (usize, PastLenLimits);
    fn next(&mut self) -> Option<Self::Item> {
        let (index, local) = self
            .0
            .iter_mut()
            .enumerate()
            .min_by_key(|(_, local)| local.next.state.updates_this_frame)?;

        let next = (local.next.state.id as usize, local.next.limits);

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

impl Direction {
    pub const NOT_LOG: Self = Self::Forward { log: false };
    pub const FORWARD_LOG: Self = Self::Forward { log: true };
    pub fn is_forward(self) -> bool {
        self != Self::BackwardLog
    }
    pub fn is_log(self) -> bool {
        self != Self::NOT_LOG
    }
}
