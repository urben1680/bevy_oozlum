use core::num::NonZeroU64;
use std::{error::Error, fmt::{Debug, Display}, num::NonZeroU32, sync::atomic::AtomicU32};

use bevy::{
    ecs::{
        change_detection::{MaybeLocation, Mut}, component::{ComponentId, Tick}, error::BevyError, resource::Resource, schedule::Schedules, system::{ReadOnlySystemParam, Res, SystemMeta, SystemParam, SystemParamValidationError}, world::{unsafe_world_cell::UnsafeWorldCell, World}
    },
    log::{info, warn},
    reflect::{std_traits::ReflectDefault, Reflect}, utils::Parallel,
};

use crate::{log::PreUpdateVariant, prelude::RevUpdate, undo_redo::{BundleIdOfOpCache, RevDespawnCleaner, UndoRedoBuffer}};

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
pub struct RevMeta {
    #[reflect(skip_serializing)]
    past_end: u64,
    now: u64,
    future_end: u64,
    max_world_states: Option<NonZeroU64>,
    direction: RunningOrRan,
    queue: Option<Queue>,
    log_exits: u64,
    past_len_ids: AtomicU32,
    past_len_ids_cleared: u64,
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

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "serialize",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum RevDirection {
    Forward { log: bool },
    BackwardLog,
}

// SAFETY: todo
unsafe impl SystemParam for RevDirection {
    type Item<'world, 'state> = Self;
    type State = ComponentId;
    fn init_state(world: &mut World) -> Self::State {
        <Res<RevMeta> as SystemParam>::init_state(world)
    }
    fn init_access(
        state: &Self::State,
        system_meta: &mut SystemMeta,
        component_access_set: &mut bevy::ecs::query::FilteredAccessSet<ComponentId>,
        world: &mut World,
    ) {
        <Res<RevMeta> as SystemParam>::init_access(state, system_meta, component_access_set, world);
    }
    // todo: update implementation and doc for bevy 0.16 as the behavior of Res changes then again
    unsafe fn validate_param(
        &mut component_id: &mut Self::State,
        _system_meta: &SystemMeta,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        let ptr = unsafe {
            // SAFETY: Read-only access is registered in init_state for this id and ptr read access is finished before return.
            world.get_resource_by_id(component_id)
        };
        ptr.map(|ptr| unsafe {
            // SAFETY: todo
            ptr.deref::<RevMeta>()
        })
        .ok_or(SystemParamValidationError::invalid::<RevDirection>(
            "RevMeta does not exist",
        ))?
        .get_running_direction()
        .ok_or(SystemParamValidationError::invalid::<RevDirection>(
            "RevMeta is not in a running direction",
        ))
        .map(|_| ())
    }
    unsafe fn get_param<'world, 'state>(
        &mut component_id: &'state mut Self::State,
        _system_meta: &SystemMeta,
        world: UnsafeWorldCell<'world>,
        _change_tick: Tick,
    ) -> Self::Item<'world, 'state> {
        let ptr = unsafe {
            // SAFETY: Read-only access is registered in init_state for this id and ptr read access is finished before return.
            world.get_resource_by_id(component_id)
        };
        ptr.map(|ptr| unsafe {
            // SAFETY: todo
            ptr.deref::<RevMeta>()
        })
        .unwrap()
        .running_direction()
    }
}

// SAFETY: only reads RevMeta resource
unsafe impl ReadOnlySystemParam for RevDirection {}

impl Display for RevDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            RevDirection::NOT_LOG => write!(f, "Forward (not log)"),
            RevDirection::FORWARD_LOG => write!(f, "Forward (log)"),
            RevDirection::BackwardLog => write!(f, "Backward (log)"),
        }
    }
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
enum RunningOrRan {
    Running(RevDirection),
    Ran(RevDirection),
    Pause {
        after_log: bool
    }
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
enum Queue {
    Run(RevDirection),
    Pause,
    ClearThenRun,
    ClearThenPause
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
struct PastLenUpdate {
    state: PastLenState,
    limits: PastLenLimits,
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PastLenState {
    id: u64,
    updates_this_frame: NonZeroU32,
    log_exits: u64,
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PastLenLimits {
    backward: u64,
    forward: u64,
    last_update: MaybeLocation,
}

impl PastLenLimits {
    #[track_caller]
    pub(crate) fn not_log_limit(backward: u64) -> Self {
        Self {
            backward,
            forward: u64::MAX,
            last_update: MaybeLocation::caller(),
        }
    }
    #[track_caller]
    pub(crate) fn log_limits(backward: u64, forward: u64) -> Self {
        Self {
            backward,
            forward,
            last_update: MaybeLocation::caller(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonLogNow(pub(crate) u64);

impl NonLogNow {
    pub fn get(self) -> u64 {
        self.0
    }
}

// todo: make PreLogUpdate a systemparam with this as the state and a resource to compare from
// so RevMeta may be used mutably
// todo: does PastLenLog still need to offer this?
// todo: if not with PastLenLog, how do other logs get this outside systems?
// integrate in every log, but then needs meta in any case
// integrated does not play well when you need to drain, especially if you need a past_len offset
// does not play well for local rollback
// idea: logs have an extra method taking RevMeta to do the pre-update ops
// variants: truncate, drain future, drain past
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub(crate) struct PreUpdateState {
    past_len_ids_cleared: u64,
    log_exits: u64,
}

impl PreUpdateState {
    pub(crate) const fn new() -> Self {
        Self { past_len_ids_cleared: 0, log_exits: 0 }
    }
    pub(crate) fn check(&mut self, meta: &RevMeta) -> PreUpdateVariant {
        if self.past_len_ids_cleared < meta.past_len_ids_cleared {
            self.past_len_ids_cleared = meta.past_len_ids_cleared;
            self.log_exits = meta.log_exits;
            PreUpdateVariant::DropLog
        } else if meta.direction == RunningOrRan::Running(RevDirection::NOT_LOG) 
            || self.log_exits < meta.log_exits
        {
            self.log_exits = meta.log_exits;
            PreUpdateVariant::DropFuture
        } else {
            PreUpdateVariant::Nothing
        }
    }
}

impl RevMeta {
    pub(crate) const EXPECT_IN_WORLD: &'static str = "RevMeta does not exist";
    pub(crate) const EXPECT_RUNNING: &'static str = "RevMeta is not in a running direction";
    pub fn new(max_world_states: Option<NonZeroU64>, paused: bool) -> Self {
        let direction = if paused {
            RunningOrRan::Pause { after_log: false }
        } else {
            RunningOrRan::Ran(RevDirection::NOT_LOG)
        };
        Self {
            past_end: 0,
            now: 0,
            future_end: 0,
            max_world_states,
            direction,
            queue: None,
            log_exits: 0,
            past_len_ids: AtomicU32::new(0),
            past_len_ids_cleared: 0,
            past_len_updates: Parallel::default(),
            past_len_limits: Vec::new(),
        }
    }
    pub fn running_direction(&self) -> RevDirection {
        self.get_running_direction().unwrap()
    }
    pub fn get_running_direction(&self) -> Option<RevDirection> {
        match self.direction {
            RunningOrRan::Running(direction) => Some(direction),
            _ => None
        }
    }
    pub fn get_ran_direction(&self) -> Option<RevDirection> {
        match self.direction {
            RunningOrRan::Ran(direction) => Some(direction),
            _ => None
        }
    }
    pub fn paused(&self) -> bool {
        matches!(self.direction, RunningOrRan::Pause { .. })
    }
    pub fn future_end(&self) -> u64 {
        self.future_end
    }
    pub const fn now(&self) -> u64 {
        self.now
    }
    pub fn non_log_now(&self) -> Option<NonLogNow> {
        matches!(self.direction, RunningOrRan::Running(RevDirection::NOT_LOG))
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
    fn clear(&mut self) {
        assert!(!matches!(self.direction, RunningOrRan::Running(_)));
        self.past_end = 0;
        self.now = 0;
        self.future_end = 0;
        self.past_len_ids_cleared += *self.past_len_ids.get_mut() as u64;
        self.past_len_ids = AtomicU32::new(0);
        self.past_len_updates.clear();
        self.past_len_limits.clear();
        self.log_exits = 0;
    }
    pub fn queue(&mut self, direction: RevDirection) {
        self.queue = Some(Queue::Run(direction));
    }
    pub fn queue_pause(&mut self) {
        self.queue = Some(Queue::Pause);
    }
    pub fn queue_clear(&mut self, then_pause: bool) {
        let queue = if then_pause { Queue::ClearThenPause } else { Queue::ClearThenRun };
        self.queue = Some(queue);
    }
    pub fn try_run_rev_update(world: &mut World) -> Result<(), BevyError> {
        Self::try_run_rev_update_typed_err(world).map_err(Into::into)
    }
    fn try_run_rev_update_typed_err(world: &mut World) -> Result<(), TryRunRevUpdateError> {
        world.try_schedule_scope(RevUpdate, |world, schedule| {
            let meta = world.remove_resource::<Self>();
            meta_or_schedule_presence::<false>(world, true);
            meta_or_schedule_presence::<true>(world, meta.is_some());
            let Some(meta) = meta else {
                return Ok(());
            };

            // init other resources
            world.init_resource::<RevDespawnCleaner>();
            world.init_resource::<BundleIdOfOpCache>();
            let buffer = world.get_resource_or_init::<UndoRedoBuffer>();
            if !buffer.is_empty() {
                return Err(TryRunRevUpdateError::UndoRedoBufferNotEmptyBeforeUpdate {
                    meta,
                    buffer: world.remove_resource::<UndoRedoBuffer>().unwrap(),
                })?;
            }

            // update RevMeta
            let mut pre_log_update = PreUpdateVariant::Nothing;
            let update_result = meta.update(|meta, pre_log_update1, _| {
                pre_log_update = pre_log_update1;
                world.insert_resource(meta);
                schedule.run(world);
                world.remove_resource::<Self>()
            });

            // finalize reversibly despawned entities
            match update_result {
                Ok(meta) => meta.rev_despawn_cleaner_update_and_meta_insert(world, pre_log_update, Vec::new()),
                Err(RevMetaUpdateErr::AlreadyRunning { meta, direction }) => Err(
                    TryRunRevUpdateError::AlreadyRunning { meta, direction }
                ),
                Err(RevMetaUpdateErr::RevMetaNotReturned) => Err(
                    TryRunRevUpdateError::RevMetaRemovedAfterRun
                ),
                Err(RevMetaUpdateErr::PastLenLogsMissed { meta, past_len_logs_missed }) => {
                    meta.rev_despawn_cleaner_update_and_meta_insert(world, pre_log_update, past_len_logs_missed)
                }
            }
        }).unwrap_or_else(|_| {
            let meta_exists = world.contains_resource::<RevMeta>();
            meta_or_schedule_presence::<false>(world, false);
            meta_or_schedule_presence::<true>(world, meta_exists);
            Ok(())
        })
    }

    fn rev_despawn_cleaner_update_and_meta_insert(
        self, 
        world: &mut World,
        pre_log_update: PreUpdateVariant,
        past_len_logs_missed: Vec<PastLenLogMissed>
    ) -> Result<(), TryRunRevUpdateError> {
        let ok_if_present = world.try_resource_scope::<RevDespawnCleaner, _>(|world, mut rev_despawn_cleaner| {
            rev_despawn_cleaner.update(&self, world, pre_log_update).is_ok()
        });
        match ok_if_present {
            Some(true) if past_len_logs_missed.is_empty() => {
                world.insert_resource(self);
                Ok(())
            },
            Some(true) => Err(TryRunRevUpdateError::AfterRunErrors { 
                meta: self, 
                past_len_logs_missed, 
                rev_despawn_cleaner_out_of_log: false, 
                rev_despawn_cleaner_removed: false 
            }),
            Some(false) => Err(TryRunRevUpdateError::AfterRunErrors { 
                meta: self,
                past_len_logs_missed,
                rev_despawn_cleaner_out_of_log: true,
                rev_despawn_cleaner_removed: false 
            }),
            None => Err(TryRunRevUpdateError::AfterRunErrors { 
                meta: self,
                past_len_logs_missed,
                rev_despawn_cleaner_out_of_log: false,
                rev_despawn_cleaner_removed: true 
            })
        }
    }

    pub fn update(mut self, c: impl FnOnce(Self, PreUpdateVariant, RevDirection) -> Option<Self>)
     -> Result<Self, RevMetaUpdateErr> {
        // get direction that ran previously
        let (ran, was_log) = match self.direction {
            RunningOrRan::Ran(direction) => {
                (Some(direction), direction.is_log())
            },
            RunningOrRan::Pause { after_log } => {
                (None, after_log)
            },
            RunningOrRan::Running(direction) => return Err(
                RevMetaUpdateErr::AlreadyRunning { meta: self, direction }
            )
        };

        // get queued direction
        let (queue, pre_log_update) = match self.queue.take() {
            Some(Queue::Run(RevDirection::NOT_LOG)) if was_log => {
                self.log_exits = self.log_exits.checked_add(1).unwrap();
                (Some(RevDirection::NOT_LOG), PreUpdateVariant::DropFuture)
            }
            Some(Queue::Run(direction)) => (Some(direction), PreUpdateVariant::Nothing),
            Some(Queue::Pause) => (None, PreUpdateVariant::Nothing),
            Some(Queue::ClearThenRun) => {
                self.clear();
                (Some(RevDirection::NOT_LOG), PreUpdateVariant::DropLog)
            },
            Some(Queue::ClearThenPause) => {
                self.clear();
                (None, PreUpdateVariant::DropLog)
            },
            None => (None, PreUpdateVariant::DropLog)
        };

        // take queue or fall back to previous direction, return None if no direction from both
        let Some(queue_or_ran) = ran.or(queue) else {
            return Ok(self);
        };
        
        // update frame fields or return None if pause is queued or reached end of log
        let direction = match queue_or_ran {
            RevDirection::NOT_LOG => {
                self.now += 1;
                self.future_end = self.now;
                if let Some(max_world_states) = self.max_world_states.map(NonZeroU64::get) {
                    // include equality here as the present state has to be added to the comparision
                    if self.past_len() >= max_world_states {
                        self.past_end = self.now + 1 - max_world_states;
                    }
                }
                RevDirection::NOT_LOG
            },
            RevDirection::FORWARD_LOG if self.now < self.future_end => {
                self.now = self.now + 1;
                RevDirection::FORWARD_LOG
            },
            RevDirection::BackwardLog if self.now > self.past_end => {
                self.now = self.now - 1;
                RevDirection::BackwardLog
            },
            _ => {
                self.direction = RunningOrRan::Pause { after_log: true };
                return Ok(self);
            }
        };

        // set running direction, call closure, set ran direction
        self.direction = RunningOrRan::Running(direction);
        let Some(mut meta) = c(self, pre_log_update, direction) else {
            return Err(RevMetaUpdateErr::RevMetaNotReturned);
        };
        meta.direction = RunningOrRan::Ran(direction);

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
        state: &mut Option<PastLenState>,
        last_update: u64,
    ) -> PreUpdateVariant {
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
            Some(state) => {
                if state.id < self.past_len_ids_cleared {
                    *state = new_state();
                    PreUpdateVariant::DropLog
                } else if state.log_exits < self.log_exits {
                    state.log_exits = self.log_exits;
                    PreUpdateVariant::DropFuture
                } else if last_update == self.now {
                    state.updates_this_frame = state.updates_this_frame.checked_add(1).unwrap();
                    PreUpdateVariant::Nothing
                } else {
                    state.updates_this_frame = NonZeroU32::MIN;
                    PreUpdateVariant::Nothing
                }
            }
            None => {
                *state = Some(new_state());
                PreUpdateVariant::Nothing
            },
        }
    }
    pub(super) fn push_past_len_update(&self, state: PastLenState, limits: PastLenLimits) {
        self.past_len_updates
            .borrow_local_mut()
            .push(PastLenUpdate { state, limits });
    }
}

fn meta_or_schedule_presence<const META: bool>(world: &mut World, exists: bool) {
    #[derive(Resource, Clone, Copy)]
    struct Existed<const META: bool>(bool);

    if exists {
        world.insert_resource(Existed::<META>(true));
        return;
    }

    let existed = world.remove_resource::<Existed::<META>>();
    world.insert_resource(Existed::<META>(false));
    match existed {
        None if META => info!(
            "resource RevMeta does not exist yet, schedule RevUpdate will not be run until it is inserted"
        ),
        None => info!(
            "schedule RevUpdate does not exist yet, it will not be run until it is inserted"
        ),
        Some(Existed(true)) if META => info!(
            "resource RevMeta was removed, reversible schedule RevUpdate will not be run until it is inserted again"
        ),
        Some(Existed(true)) => info!(
            "schedule RevUpdate was removed, it will not be run until it is inserted again"
        ),
        Some(Existed(false)) => {}
    };
}

pub enum RevMetaUpdateErr {
    AlreadyRunning {
        meta: RevMeta,
        direction: RevDirection
    },
    RevMetaNotReturned,
    PastLenLogsMissed {
        meta: RevMeta,
        past_len_logs_missed: Vec<PastLenLogMissed>
    }
}

#[derive(Debug)]
pub struct PastLenLogMissed {
    internal_id: u64,
    missed_forward: bool,
    last_update: MaybeLocation,
}

#[derive(Debug)]
enum TryRunRevUpdateError {
    UndoRedoBufferNotEmptyBeforeUpdate {
        meta: RevMeta,
        buffer: UndoRedoBuffer,
    },
    AlreadyRunning {
        meta: RevMeta,
        direction: RevDirection,
    },
    RevMetaRemovedAfterRun, // todo: MaybeLocation when that is tracked by bevy
    AfterRunErrors {
        meta: RevMeta,
        past_len_logs_missed: Vec<PastLenLogMissed>,
        rev_despawn_cleaner_out_of_log: bool,
        rev_despawn_cleaner_removed: bool
    }
}

impl Display for TryRunRevUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UndoRedoBufferNotEmptyBeforeUpdate { meta, buffer } => write!(
                f,
                "the resource containing buffered UndoRedo implementors was not empty, it contained the following types:\n{buffer:?}\n{meta:?}"
            ),
            Self::AlreadyRunning { meta, direction } => write!(
                f,
                "RevMeta is already running at {direction:?}\n{meta:?}"
            ),
            Self::RevMetaRemovedAfterRun => write!(
                f,
                "RevMeta was removed while running"
            ),
            Self::AfterRunErrors { meta, past_len_logs_missed, rev_despawn_cleaner_out_of_log, rev_despawn_cleaner_removed } => {
                if !past_len_logs_missed.is_empty() {
                    write!(
                        f,
                        "PastLenLog instances did not run when they were expected to:\n{past_len_logs_missed:?}\n"
                    )?;
                }
                if *rev_despawn_cleaner_out_of_log {
                    write!(
                        f,
                        "The resource that finally despawns entities that were reversibly marked for despawn unexpectedly went out-of-log\n"
                    )?;
                }
                if *rev_despawn_cleaner_removed {
                    write!(
                        f,
                        "The resource that finally despawns entities that were reversibly marked for despawn was removed\n"
                    )?;
                }
                write!(
                    f,
                    "{meta:?}"
                )
            }
        }
    }
}

impl Error for TryRunRevUpdateError {}

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

impl RevDirection {
    pub const NOT_LOG: Self = Self::Forward { log: false };
    pub const FORWARD_LOG: Self = Self::Forward { log: true };
    pub fn is_forward(self) -> bool {
        self != Self::BackwardLog
    }
    pub fn is_log(self) -> bool {
        self != Self::NOT_LOG
    }
}
