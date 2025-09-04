use core::num::NonZeroU64;
use std::{
    error::Error,
    fmt::{Debug, Display},
};

use bevy::{
    ecs::{
        component::{ComponentId, Tick},
        error::BevyError,
        resource::Resource,
        system::{ReadOnlySystemParam, Res, SystemMeta, SystemParam, SystemParamValidationError},
        world::{World, unsafe_world_cell::UnsafeWorldCell},
    },
    log::info,
    reflect::Reflect,
};

use crate::{
    log::{PastLenLogLimits, PastLenLogMissed, PastLenState, PreUpdateVariant},
    prelude::RevUpdate,
    undo_redo::{BundleIdOfOpCache, RevDespawnCleaner, UndoRedoBuffer},
};

#[derive(Reflect, Resource, Debug)]
pub struct RevMeta {
    #[reflect(skip_serializing)]
    past_end: u64,
    now: u64,
    future_end: u64,
    pub max_world_states: Option<NonZeroU64>,
    direction: RunningOrRan,
    queue: Option<Queue>,
    log_exits: u64,
    log_clears: u64,
    past_len_limits: PastLenLogLimits,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_WORLD_STATES, Self::DEFAULT_PAUSED)
    }
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RevDirection {
    Forward { log: bool },
    BackwardLog,
}

impl RevDirection {
    pub const NOT_LOG: Self = Self::Forward { log: false };
    pub const FORWARD_LOG: Self = Self::Forward { log: true };
    pub fn is_backward_log(self) -> bool {
        self == Self::BackwardLog
    }
    pub fn is_forward(self) -> bool {
        self != Self::BackwardLog
    }
    pub fn is_forward_log(self) -> bool {
        self == Self::FORWARD_LOG
    }
    pub fn is_not_log(self) -> bool {
        self == Self::NOT_LOG
    }
    pub fn is_log(self) -> bool {
        self != Self::NOT_LOG
    }
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
    Pause { after_log: bool },
}

#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
enum Queue {
    Run(RevDirection),
    Pause,
    ClearThenRun,
    ClearThenPause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonLogNow(pub(crate) u64);

impl NonLogNow {
    pub fn get(self) -> u64 {
        self.0
    }
}

impl RevMeta {
    pub(crate) const DEFAULT_MAX_WORLD_STATES: Option<NonZeroU64> = Some(NonZeroU64::MIN);
    pub(crate) const DEFAULT_PAUSED: bool = false;
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
            log_clears: 0,
            past_len_limits: PastLenLogLimits::new(),
        }
    }
    #[cfg(test)]
    pub(crate) fn running_new() -> Self {
        Self {
            past_end: 0,
            now: 0,
            future_end: 0,
            max_world_states: None,
            direction: RunningOrRan::Running(RevDirection::NOT_LOG),
            queue: None,
            log_exits: 0,
            log_clears: 0,
            past_len_limits: PastLenLogLimits::new(),
        }
    }
    pub fn running_direction(&self) -> RevDirection {
        self.get_running_direction().unwrap()
    }
    pub fn get_running_direction(&self) -> Option<RevDirection> {
        match self.direction {
            RunningOrRan::Running(direction) => Some(direction),
            _ => None,
        }
    }
    pub fn get_ran_direction(&self) -> Option<RevDirection> {
        match self.direction {
            RunningOrRan::Ran(direction) => Some(direction),
            _ => None,
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
    pub fn log_exits(&self) -> u64 {
        self.log_exits
    }
    pub fn log_clears(&self) -> u64 {
        self.log_clears
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
    pub fn end_of_log_backward(&self) -> bool {
        self.now == self.past_end
    }
    pub fn end_of_log_forward(&self) -> bool {
        self.now == self.future_end
    }
    fn clear(&mut self) {
        self.past_end = 0;
        self.now = 0;
        self.future_end = 0;
        self.log_clears += 1;
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
        let queue = if then_pause {
            Queue::ClearThenPause
        } else {
            Queue::ClearThenRun
        };
        self.queue = Some(queue);
    }
    pub fn try_run_rev_update(world: &mut World) -> Result<(), BevyError> {
        Self::try_run_rev_update_typed_err(world).map_err(Into::into)
    }
    fn try_run_rev_update_typed_err(world: &mut World) -> Result<(), TryRunRevUpdateError> {
        world
            .try_schedule_scope(RevUpdate, |world, schedule| {
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

                // update RevMeta and RevDespawnCleaner
                let mut despawn_cleaner_missing = !world.contains_resource::<RevDespawnCleaner>();
                let mut meta_stopped_running_early = false;
                let mut despawn_cleaner_out_of_log = false;
                let update_result = meta.update(|meta, _| {
                    world.insert_resource(meta);
                    schedule.run(world);
                    let cleaner_scope_option = world.try_resource_scope::<RevDespawnCleaner, _>(
                        |world, mut rev_despawn_cleaner| {
                            despawn_cleaner_missing = false;
                            rev_despawn_cleaner.update_get_meta(
                                world,
                                &mut meta_stopped_running_early,
                                &mut despawn_cleaner_out_of_log,
                            )
                        },
                    );
                    despawn_cleaner_missing = cleaner_scope_option.is_none();
                    cleaner_scope_option.flatten()
                });

                // finalize reversibly despawned entities TODO: muss in update damit running-direction Some ist
                match update_result {
                    Ok(meta)
                        if !despawn_cleaner_missing
                            && !meta_stopped_running_early
                            && !despawn_cleaner_out_of_log =>
                    {
                        world.insert_resource(meta);
                        Ok(())
                    }
                    Ok(meta) => Err(TryRunRevUpdateError::AfterRunErrors {
                        meta: Some(meta),
                        meta_stopped_running_early,
                        past_len_logs_missed: Vec::new(),
                        despawn_cleaner_out_of_log,
                        despawn_cleaner_missing,
                    }),
                    Err(RevMetaUpdateErr::AlreadyRunning { meta, direction }) => {
                        Err(TryRunRevUpdateError::AlreadyRunning { meta, direction })
                    }
                    Err(RevMetaUpdateErr::RevMetaNotReturned) => {
                        Err(TryRunRevUpdateError::AfterRunErrors {
                            meta: None,
                            meta_stopped_running_early,
                            past_len_logs_missed: Vec::new(),
                            despawn_cleaner_out_of_log,
                            despawn_cleaner_missing,
                        })
                    }
                    Err(RevMetaUpdateErr::PastLenLogsMissed {
                        meta,
                        past_len_logs_missed,
                    }) => Err(TryRunRevUpdateError::AfterRunErrors {
                        meta: Some(meta),
                        meta_stopped_running_early,
                        past_len_logs_missed,
                        despawn_cleaner_out_of_log,
                        despawn_cleaner_missing,
                    }),
                }
            })
            .unwrap_or_else(|_| {
                let meta_exists = world.contains_resource::<RevMeta>();
                meta_or_schedule_presence::<false>(world, false);
                meta_or_schedule_presence::<true>(world, meta_exists);
                Ok(())
            })
    }

    pub fn update(
        mut self,
        c: impl FnOnce(Self, RevDirection) -> Option<Self>,
    ) -> Result<Self, RevMetaUpdateErr> {
        // get direction that ran previously
        let (ran, after_log) = match self.direction {
            RunningOrRan::Ran(RevDirection::NOT_LOG) => (Some(RevDirection::NOT_LOG), false),
            RunningOrRan::Ran(RevDirection::FORWARD_LOG) => (
                (!self.end_of_log_forward()).then_some(RevDirection::FORWARD_LOG),
                true,
            ),
            RunningOrRan::Ran(RevDirection::BackwardLog) => (
                (!self.end_of_log_backward()).then_some(RevDirection::BackwardLog),
                true,
            ),
            RunningOrRan::Pause { after_log } => (None, after_log),
            RunningOrRan::Running(direction) => {
                return Err(RevMetaUpdateErr::AlreadyRunning {
                    meta: self,
                    direction,
                });
            }
        };

        // get queued direction
        let queue = match self.queue.take() {
            None => None,
            Some(Queue::Run(RevDirection::NOT_LOG)) => {
                if after_log {
                    self.log_exits = self.log_exits.checked_add(1).unwrap();
                }
                Some(RevDirection::NOT_LOG)
            }
            Some(Queue::Run(RevDirection::FORWARD_LOG)) if !self.end_of_log_forward() => {
                Some(RevDirection::FORWARD_LOG)
            }
            Some(Queue::Run(RevDirection::BackwardLog)) if !self.end_of_log_backward() => {
                Some(RevDirection::BackwardLog)
            }
            Some(Queue::ClearThenRun) => {
                self.clear();
                Some(RevDirection::NOT_LOG)
            }
            Some(Queue::ClearThenPause) => {
                self.clear();
                self.direction = RunningOrRan::Pause { after_log: false };
                return Ok(self);
            }
            _ => {
                // queued log direction at end of log behaves like Queue::Pause which is matched
                // here as well
                self.direction = RunningOrRan::Pause { after_log };
                return Ok(self);
            }
        };

        // take queue or fall back to previous direction, return None if no direction from both
        let Some(queue_or_ran) = queue.or(ran) else {
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
            }
            RevDirection::FORWARD_LOG => {
                self.now = self.now + 1;
                RevDirection::FORWARD_LOG
            }
            RevDirection::BackwardLog => {
                self.now = self.now - 1;
                RevDirection::BackwardLog
            }
        };

        // set running direction, call closure, set ran direction
        self.direction = RunningOrRan::Running(direction);
        let Some(mut meta) = c(self, direction) else {
            return Err(RevMetaUpdateErr::RevMetaNotReturned);
        };
        meta.direction = RunningOrRan::Ran(direction);

        let now = meta.now;
        match meta
            .past_len_limits
            .check_past_len_limits(now, direction.is_log())
        {
            Ok(()) => Ok(meta),
            Err(past_len_logs_missed) => Err(RevMetaUpdateErr::PastLenLogsMissed {
                meta,
                past_len_logs_missed,
            }),
        }
    }
    pub(super) fn update_past_len_state(
        &self,
        state: &mut Option<PastLenState>,
        last_update: u64,
    ) -> PreUpdateVariant {
        self.past_len_limits.update_past_len_state(
            state,
            self.now == last_update,
            self.log_exits,
            self.log_clears,
        )
    }
    pub(super) fn past_len_limits(&self) -> &PastLenLogLimits {
        &self.past_len_limits
    }
}

fn meta_or_schedule_presence<const META: bool>(world: &mut World, exists: bool) {
    #[derive(Resource, Clone, Copy)]
    struct Existed<const META: bool>(bool);

    if exists {
        world.insert_resource(Existed::<META>(true));
        return;
    }

    let existed = world.remove_resource::<Existed<META>>();
    world.insert_resource(Existed::<META>(false));
    match existed {
        None if META => info!(
            "resource RevMeta does not exist yet, schedule RevUpdate will not be run until it is inserted"
        ),
        None => {
            info!("schedule RevUpdate does not exist yet, it will not be run until it is inserted")
        }
        Some(Existed(true)) if META => info!(
            "resource RevMeta was removed, reversible schedule RevUpdate will not be run until it is inserted again"
        ),
        Some(Existed(true)) => {
            info!("schedule RevUpdate was removed, it will not be run until it is inserted again")
        }
        Some(Existed(false)) => {}
    };
}

#[derive(Debug)]
pub enum RevMetaUpdateErr {
    AlreadyRunning {
        meta: RevMeta,
        direction: RevDirection,
    },
    RevMetaNotReturned,
    PastLenLogsMissed {
        meta: RevMeta,
        past_len_logs_missed: Vec<PastLenLogMissed>,
    },
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
    AfterRunErrors {
        meta: Option<RevMeta>,
        meta_stopped_running_early: bool,
        past_len_logs_missed: Vec<PastLenLogMissed>,
        despawn_cleaner_out_of_log: bool,
        despawn_cleaner_missing: bool,
    },
}

impl Display for TryRunRevUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UndoRedoBufferNotEmptyBeforeUpdate { meta, buffer } => write!(
                f,
                "the resource containing buffered UndoRedo implementors was not empty, it contained the following types:\n{buffer:?}\n{meta:?}"
            ),
            Self::AlreadyRunning { meta, direction } => {
                write!(f, "RevMeta is already running at {direction:?}\n{meta:?}")
            }
            Self::AfterRunErrors {
                meta,
                meta_stopped_running_early,
                past_len_logs_missed,
                despawn_cleaner_out_of_log,
                despawn_cleaner_missing,
            } => {
                if *meta_stopped_running_early {
                    write!(f, "RevMeta stopped running early\n")?;
                }
                if !past_len_logs_missed.is_empty() {
                    write!(
                        f,
                        "PastLenLog instances did not run when they were expected to:\n{past_len_logs_missed:?}\n"
                    )?;
                }
                if *despawn_cleaner_out_of_log {
                    write!(
                        f,
                        "the resource that finally despawns entities that were reversibly marked for despawn unexpectedly went out-of-log\n"
                    )?;
                }
                if *despawn_cleaner_missing {
                    write!(
                        f,
                        "the resource that finally despawns entities that were reversibly marked for despawn was removed\n"
                    )?;
                }
                if meta.is_none() {
                    write!(f, "RevMeta was removed from the world while running")?;
                }
                if let Some(meta) = meta {
                    write!(f, "{meta:?}")?;
                }
                Ok(())
            }
        }
    }
}

impl Error for TryRunRevUpdateError {}

#[cfg(test)]
mod test {
    use super::*;

    enum Queue {
        None,
        Pause,
        NotLog,
        ForwardLog,
        BackwardLog,
        ClearThenPause,
        ClearThenRun,
    }

    struct RunValues {
        past_end: u64,
        now: u64,
        future_end: u64,
        log_exits: u64,
        log_clears: u64,
        direction: RevDirection,
    }

    impl RevMeta {
        fn update_assert(&mut self, queue: Queue, values: Option<RunValues>) {
            match queue {
                Queue::None => {}
                Queue::Pause => self.queue_pause(),
                Queue::NotLog => self.queue(RevDirection::NOT_LOG),
                Queue::ForwardLog => self.queue(RevDirection::FORWARD_LOG),
                Queue::BackwardLog => self.queue(RevDirection::BackwardLog),
                Queue::ClearThenPause => self.queue_clear(true),
                Queue::ClearThenRun => self.queue_clear(false),
            }
            let mut ran = false;
            let values_is_some = values.is_some();
            let meta = core::mem::replace(self, RevMeta::new(None, true));
            *self = meta
                .update(|meta, direction| {
                    ran = true;
                    let values = values.unwrap();
                    assert_eq!(meta.past_end(), values.past_end);
                    assert_eq!(meta.now(), values.now);
                    assert_eq!(meta.future_end(), values.future_end);
                    assert_eq!(meta.log_exits(), values.log_exits);
                    assert_eq!(meta.log_clears(), values.log_clears);
                    assert_eq!(direction, values.direction);
                    Some(meta)
                })
                .unwrap();
            assert_eq!(ran, values_is_some);
        }
    }

    #[test]
    fn traverses_log() {
        let mut meta = RevMeta::new(NonZeroU64::new(5), false);
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 0,
                now: 1,
                future_end: 1,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::NotLog,
            Some(RunValues {
                past_end: 0,
                now: 2,
                future_end: 2,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 0,
                now: 3,
                future_end: 3,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 0,
                now: 4,
                future_end: 4,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 1,
                now: 5,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::BackwardLog,
            Some(RunValues {
                past_end: 1,
                now: 4,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 1,
                now: 3,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(
            Queue::BackwardLog,
            Some(RunValues {
                past_end: 1,
                now: 2,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 1,
                now: 1,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(Queue::None, None);
        meta.update_assert(Queue::BackwardLog, None);
        meta.update_assert(
            Queue::ForwardLog,
            Some(RunValues {
                past_end: 1,
                now: 2,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::FORWARD_LOG,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 1,
                now: 3,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::FORWARD_LOG,
            }),
        );
        meta.update_assert(
            Queue::ForwardLog,
            Some(RunValues {
                past_end: 1,
                now: 4,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::FORWARD_LOG,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 1,
                now: 5,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::FORWARD_LOG,
            }),
        );
        meta.update_assert(Queue::None, None);
        meta.update_assert(Queue::ForwardLog, None);
        meta.update_assert(
            Queue::BackwardLog,
            Some(RunValues {
                past_end: 1,
                now: 4,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 1,
                now: 3,
                future_end: 5,
                log_exits: 0,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(Queue::Pause, None);
        meta.update_assert(
            Queue::NotLog,
            Some(RunValues {
                past_end: 1,
                now: 4,
                future_end: 4,
                log_exits: 1,
                log_clears: 0,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::BackwardLog,
            Some(RunValues {
                past_end: 1,
                now: 3,
                future_end: 4,
                log_exits: 1,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 1,
                now: 2,
                future_end: 4,
                log_exits: 1,
                log_clears: 0,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(Queue::ClearThenPause, None);
        meta.update_assert(
            Queue::NotLog,
            Some(RunValues {
                past_end: 0,
                now: 1,
                future_end: 1,
                log_exits: 0,
                log_clears: 1,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 0,
                now: 2,
                future_end: 2,
                log_exits: 0,
                log_clears: 1,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 0,
                now: 3,
                future_end: 3,
                log_exits: 0,
                log_clears: 1,
                direction: RevDirection::NOT_LOG,
            }),
        );
        meta.update_assert(
            Queue::BackwardLog,
            Some(RunValues {
                past_end: 0,
                now: 2,
                future_end: 3,
                log_exits: 0,
                log_clears: 1,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(
            Queue::None,
            Some(RunValues {
                past_end: 0,
                now: 1,
                future_end: 3,
                log_exits: 0,
                log_clears: 1,
                direction: RevDirection::BackwardLog,
            }),
        );
        meta.update_assert(
            Queue::ClearThenRun,
            Some(RunValues {
                past_end: 0,
                now: 1,
                future_end: 1,
                log_exits: 0,
                log_clears: 2,
                direction: RevDirection::NOT_LOG,
            }),
        );
    }

    #[test]
    fn contains_returns_expected() {
        let mut meta = RevMeta::new(None, true);
        meta.past_end = 1;
        meta.now = 3;
        meta.future_end = 5;

        assert_eq!(meta.contains(0), false, "{meta:#?}");
        assert_eq!(meta.contains(1), true, "{meta:#?}");
        assert_eq!(meta.contains(2), true, "{meta:#?}");
        assert_eq!(meta.contains(3), true, "{meta:#?}");
        assert_eq!(meta.contains(4), true, "{meta:#?}");
        assert_eq!(meta.contains(5), true, "{meta:#?}");
        assert_eq!(meta.contains(6), false, "{meta:#?}");

        assert_eq!(meta.past_contains(0), false, "{meta:#?}");
        assert_eq!(meta.past_contains(1), true, "{meta:#?}");
        assert_eq!(meta.past_contains(2), true, "{meta:#?}");
        assert_eq!(meta.past_contains(3), false, "{meta:#?}");
        assert_eq!(meta.past_contains(4), false, "{meta:#?}");
        assert_eq!(meta.past_contains(5), false, "{meta:#?}");
        assert_eq!(meta.past_contains(6), false, "{meta:#?}");

        assert_eq!(meta.future_contains(0), false, "{meta:#?}");
        assert_eq!(meta.future_contains(1), false, "{meta:#?}");
        assert_eq!(meta.future_contains(2), false, "{meta:#?}");
        assert_eq!(meta.future_contains(3), false, "{meta:#?}");
        assert_eq!(meta.future_contains(4), true, "{meta:#?}");
        assert_eq!(meta.future_contains(5), true, "{meta:#?}");
        assert_eq!(meta.future_contains(6), false, "{meta:#?}");
    }
}
