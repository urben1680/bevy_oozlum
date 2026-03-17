use crate::{
    log::{PreUpdateKind, UpdateLogLimits, UpdateLogMissed, UpdateLogState},
    prelude::RevUpdate,
    undo_redo::{DespawnCleanerErr, UndoRedoBuffer, update_spawn_despawn},
};
use bevy_ecs::{
    change_detection::MaybeLocation,
    resource::Resource,
    system::{RunSystemError, SystemParamValidationError},
    world::World,
};
use bevy_log::info;
use core::{
    error::Error,
    fmt::{Debug, Display},
    num::NonZeroU64,
};
use std::{borrow::Cow, panic::Location};

#[cfg(test)]
mod test;

#[derive(Resource, Debug)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub struct RevMeta {
    past_end: u64,
    now: u64,
    future_end: u64,
    max_past_len: NonZeroU64,
    direction: RunningOrRan,
    queue: Option<RevQueue>,
    log_exits: u64,
    log_clears: u64,
    update_log_limits: UpdateLogLimits,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Self::DEFAULT_MAX_PAST_LEN, Self::DEFAULT_PAUSED)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub enum RevDirection {
    Forward { meta_past_len: MetaPastLen },
    ForwardLog,
    BackwardLog,
}

impl RevDirection {
    pub(crate) const FORWARD_MIN: Self = Self::Forward {
        meta_past_len: MetaPastLen(NonZeroU64::MIN),
    };
    pub fn is_forward(self) -> bool {
        !self.is_backward()
    }
    pub fn is_backward(self) -> bool {
        matches!(self, Self::BackwardLog)
    }
    pub fn is_log(self) -> bool {
        !self.is_not_log()
    }
    pub fn is_not_log(self) -> bool {
        matches!(self, Self::Forward { .. })
    }
    pub fn past_len(self) -> MetaPastLen {
        match self {
            Self::Forward { meta_past_len } => meta_past_len,
            _ => panic!(),
        }
    }
    pub fn get_past_len(self) -> Option<MetaPastLen> {
        match self {
            Self::Forward { meta_past_len } => Some(meta_past_len),
            _ => None,
        }
    }
}

impl Display for RevDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            RevDirection::Forward { .. } => write!(f, "Forward"),
            RevDirection::ForwardLog => write!(f, "Forward Log"),
            RevDirection::BackwardLog => write!(f, "Backward Log"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
enum RunningOrRan {
    Running(RevDirection),
    Ran(RevDirection),
    Pause { after_log: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub enum RevQueue {
    RunForward,
    RunForwardLog,
    RunBackwardLog,
    Pause,
    ClearThenRunForward,
    ClearThenPause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub struct MetaPastLen(NonZeroU64);

impl Into<NonZeroU64> for MetaPastLen {
    fn into(self) -> NonZeroU64 {
        self.0
    }
}

impl RevMeta {
    pub(crate) const DEFAULT_MAX_PAST_LEN: NonZeroU64 = NonZeroU64::MIN;
    pub(crate) const DEFAULT_PAUSED: bool = false;
    pub fn new(max_past_len: NonZeroU64, paused: bool) -> Self {
        Self {
            past_end: 0,
            now: 0,
            future_end: 0,
            max_past_len,
            direction: RunningOrRan::Pause { after_log: false },
            queue: (!paused).then_some(RevQueue::RunForward),
            log_exits: 0,
            log_clears: 0,
            update_log_limits: UpdateLogLimits::default(),
        }
    }
    pub fn set_queue(&mut self, queue: RevQueue) {
        self.queue = Some(queue);
    }
    pub fn unset_queue(&mut self) {
        self.queue = None;
    }
    pub fn get_queue(&self) -> Option<RevQueue> {
        self.queue
    }
    pub fn set_max_past_len(&mut self, max_past_len: NonZeroU64) {
        self.max_past_len = max_past_len;
    }
    pub fn get_max_past_len(&self) -> NonZeroU64 {
        self.max_past_len
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
    /// The reversible frame of the [`RevUpdate`] schedule.
    ///
    /// When the schedule is not running, this frame can be understood as the id of the present
    /// world state.
    ///
    /// When the schedule is running, the world state of this id reflects what the systems should
    /// work towards to. For example when the schedule is running forward, `now` is increased from
    /// `n` to `n+1` and the reversible systems now have to bring the world state to from `n` to
    /// `n+1`.
    ///
    /// That is why, when the schedule is running backward, the returned value is not the same but
    /// one less than when it ran forward just previously, as the reversible systems have to bring
    /// the world from `n+1` back to `n` again.
    ///
    /// Can only be `0` at or after [`RevDirection::BackwardLog`] and is otherwise non-zero.
    pub const fn now(&self) -> u64 {
        self.now
    }
    pub fn past_end(&self) -> u64 {
        self.past_end
    }
    pub fn future_end(&self) -> u64 {
        self.future_end
    }
    pub fn past_len(&self) -> u64 {
        self.now - self.past_end
    }
    pub fn meta_past_len(&self) -> MetaPastLen {
        self.get_meta_past_len().unwrap()
    }
    pub fn get_meta_past_len(&self) -> Option<MetaPastLen> {
        match self.direction {
            RunningOrRan::Running(RevDirection::Forward { meta_past_len }) => Some(meta_past_len),
            _ => None,
        }
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
        self.now.wrapping_sub(frame).wrapping_sub(1) < self.past_len()
    }
    pub fn future_contains(&self, frame: u64) -> bool {
        self.future_end.wrapping_sub(frame) < self.future_len()
    }
    pub fn end_of_log_backward(&self) -> bool {
        self.now == self.past_end
    }
    pub fn end_of_log_forward(&self) -> bool {
        self.now == self.future_end
    }
    fn clear(&mut self) {
        self.past_end = self.now;
        self.future_end = self.now;
        self.log_clears = self.log_clears.checked_add(1).expect("todo");
        self.log_exits = 0;
        if self.update_log_limits.clear() {
            info!(
                "`RevQueue::Clear` was applied, `RevMeta::log_clears` is now {},  all `UpdateLog::id` \
                until now are invalid and will be reinitialized at their next mutation, this message \
                is only logged if any `UpdateLog` was registered since the last clear",
                self.log_clears
            )
        }
    }
    pub fn run_rev_update<'w>(
        world: &'w mut World,
    ) -> Result<(), RunSystemError> {
        world
            .try_schedule_scope(RevUpdate, |world, schedule| {
                let Some(meta) = world.remove_resource::<Self>() else {
                    return Err(TryRunRevUpdateError::MetaMissing);
                };

                let buffer = world.get_resource_or_init::<UndoRedoBuffer>();
                if !buffer.is_empty() {
                    world.insert_resource(meta);
                    return Err(TryRunRevUpdateError::UndoRedoBufferNotEmptyBeforeUpdate {
                        meta: (),
                        buffer: (),
                    });
                }

                // update RevMeta and RevDespawnCleaner
                let mut cleaner_result = Ok(());
                let meta_result = meta.update(|meta, _| {
                    world.insert_resource(meta);
                    schedule.run(world);
                    cleaner_result = update_spawn_despawn(world);
                    world.remove_resource::<RevMeta>()
                });

                // map errors
                match meta_result {
                    Ok(meta) => {
                        world.insert_resource(meta);
                        match cleaner_result {
                            Ok(()) => Ok(()),
                            Err(err) => Err(TryRunRevUpdateError::AfterRunErrors {
                                meta: None,
                                update_logs_missed: Vec::new(),
                                despawn_cleaner_err: Some(err),
                            }),
                        }
                    }
                    Err(RevMetaUpdateErr::AlreadyRunning { meta, direction }) => {
                        world.insert_resource(meta);
                        Err(TryRunRevUpdateError::AlreadyRunning {
                            meta: (),
                            direction,
                        })
                    }
                    Err(RevMetaUpdateErr::RevMetaNotReturned) => {
                        Err(TryRunRevUpdateError::AfterRunErrors {
                            meta: None,
                            update_logs_missed: Vec::new(),
                            despawn_cleaner_err: cleaner_result.err(),
                        })
                    }
                    Err(RevMetaUpdateErr::UpdateLogsMissed {
                        meta,
                        update_logs_missed,
                    }) => {
                        world.insert_resource(meta);
                        Err(TryRunRevUpdateError::AfterRunErrors {
                            meta: None,
                            update_logs_missed,
                            despawn_cleaner_err: cleaner_result.err(),
                        })
                    }
                }
            })
            .unwrap_or_else(|_| {
                if world.contains_resource::<RevMeta>() {
                    Err(TryRunRevUpdateError::ScheduleMissing)
                } else {
                    Err(TryRunRevUpdateError::ScheduleAndMetaMissing)
                }
            })
            .map_err(|err| match err {
                TryRunRevUpdateError::ScheduleMissing => TryRunRevUpdateError::ScheduleMissing,
                TryRunRevUpdateError::MetaMissing => TryRunRevUpdateError::MetaMissing,
                TryRunRevUpdateError::ScheduleAndMetaMissing => {
                    TryRunRevUpdateError::ScheduleAndMetaMissing
                }
                TryRunRevUpdateError::UndoRedoBufferNotEmptyBeforeUpdate { .. } => {
                    TryRunRevUpdateError::UndoRedoBufferNotEmptyBeforeUpdate {
                        meta: world.resource(),
                        buffer: world.resource::<UndoRedoBuffer>(),
                    }
                }
                TryRunRevUpdateError::AlreadyRunning { direction, .. } => {
                    TryRunRevUpdateError::AlreadyRunning {
                        meta: world.resource(),
                        direction,
                    }
                }
                TryRunRevUpdateError::AfterRunErrors {
                    update_logs_missed,
                    despawn_cleaner_err,
                    ..
                } => TryRunRevUpdateError::AfterRunErrors {
                    meta: world.get_resource(),
                    update_logs_missed,
                    despawn_cleaner_err,
                },
            }).map_err(TryRunRevUpdateError::to_run_system_err)
    }

    pub fn update(
        mut self,
        c: impl FnOnce(Self, RevDirection) -> Option<Self>,
    ) -> Result<Self, RevMetaUpdateErr> {
        // get direction that ran previously
        let (ran, after_log) = match self.direction {
            RunningOrRan::Ran(RevDirection::Forward { .. }) => (
                Some(RevDirection::FORWARD_MIN), // gets updated below
                false,
            ),
            RunningOrRan::Ran(RevDirection::ForwardLog) => (
                (!self.end_of_log_forward()).then_some(RevDirection::ForwardLog),
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
            Some(RevQueue::RunForward) => {
                if after_log {
                    self.log_exits += 1;
                }
                Some(RevDirection::FORWARD_MIN) // gets updated below
            }
            Some(RevQueue::RunForwardLog) if !self.end_of_log_forward() => {
                Some(RevDirection::ForwardLog)
            }
            Some(RevQueue::RunBackwardLog) if !self.end_of_log_backward() => {
                Some(RevDirection::BackwardLog)
            }
            Some(RevQueue::ClearThenRunForward) => {
                self.clear();
                Some(RevDirection::FORWARD_MIN) // gets updated below
            }
            Some(RevQueue::ClearThenPause) => {
                self.clear();
                self.direction = RunningOrRan::Pause { after_log: false };
                return Ok(self);
            }
            _ => {
                // queued log direction at end of log behaves like Some(RevQueue::Pause) which is matched
                // here as well
                self.direction = RunningOrRan::Pause { after_log };
                return Ok(self);
            }
        };

        // take queue or fall back to previous direction, return early if no direction from both
        let Some(queue_or_ran) = queue.or(ran) else {
            return Ok(self);
        };

        let direction = match queue_or_ran {
            RevDirection::Forward { .. } => {
                self.now += 1;
                self.future_end = self.now;
                let max_past_len = self.max_past_len.get();
                if self.past_len() > max_past_len {
                    self.past_end = self.now - max_past_len;
                }
                let past_len = NonZeroU64::new(self.now - self.past_end)
                    .expect("now is increased here and larger than past_end");
                RevDirection::Forward {
                    meta_past_len: MetaPastLen(past_len),
                }
            }
            RevDirection::ForwardLog => {
                self.now = self.now + 1;
                RevDirection::ForwardLog
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

        // check for `UpdateLog` instances that were missed being updated
        let now = meta.now;
        match meta
            .update_log_limits
            .update(now, meta.log_clears, direction.is_log())
        {
            Ok(()) => Ok(meta),
            Err(update_logs_missed) => Err(RevMetaUpdateErr::UpdateLogsMissed {
                meta,
                update_logs_missed,
            }),
        }
    }
    #[cfg(test)]
    pub(crate) fn update_ref(
        &mut self,
        should_run: Result<bool, UpdateLogMissed>,
        c: impl FnOnce(&mut Self, RevDirection),
    ) {
        let meta = core::mem::replace(self, RevMeta::new(NonZeroU64::MAX, true));
        match should_run {
            Ok(should_run) => {
                let mut ran = false;
                let result = meta.update(|mut meta, direction| {
                    ran = true;
                    c(&mut meta, direction);
                    Some(meta)
                });
                match result {
                    Ok(meta) => *self = meta,
                    err => panic!("unexpected {err:#?}"),
                }
                assert_eq!(ran, should_run);
            }
            Err(missed) => {
                let mut ran = false;
                let result = meta.update(|mut meta, direction| {
                    ran = true;
                    c(&mut meta, direction);
                    Some(meta)
                });
                assert_eq!(ran, true);
                match result {
                    Err(RevMetaUpdateErr::UpdateLogsMissed {
                        meta,
                        update_logs_missed,
                    }) => {
                        *self = meta;
                        assert_eq!(update_logs_missed, [missed]);
                    }
                    other => panic!("unexpected {other:#?}"),
                }
            }
        }
    }
    pub(super) fn set_update_state(
        &self,
        state: &mut Option<UpdateLogState>,
        caller: MaybeLocation<Option<&'static Location>>,
    ) -> PreUpdateKind {
        self.update_log_limits
            .set_update_state(state, self.log_exits, self.log_clears, caller)
    }
    pub(super) fn update_log_limits(&self) -> &UpdateLogLimits {
        &self.update_log_limits
    }
}

#[derive(Debug)]
pub enum RevMetaUpdateErr {
    AlreadyRunning {
        meta: RevMeta,
        direction: RevDirection,
    },
    RevMetaNotReturned,
    UpdateLogsMissed {
        meta: RevMeta,
        update_logs_missed: Vec<UpdateLogMissed>,
    },
}

#[derive(Debug)]
enum TryRunRevUpdateError<M, B> {
    ScheduleMissing,
    MetaMissing,
    ScheduleAndMetaMissing,
    UndoRedoBufferNotEmptyBeforeUpdate {
        meta: M,
        buffer: B,
    },
    AlreadyRunning {
        meta: M,
        direction: RevDirection,
    },
    AfterRunErrors {
        meta: Option<M>,
        update_logs_missed: Vec<UpdateLogMissed>,
        despawn_cleaner_err: Option<DespawnCleanerErr>,
    },
}

impl<'w, B: Debug + 'w> TryRunRevUpdateError<&'w RevMeta, B> {
    pub fn to_run_system_err(self) -> RunSystemError {
        match self {
            TryRunRevUpdateError::ScheduleMissing => RunSystemError::Skipped(
                SystemParamValidationError::skipped::<RevMeta>(Cow::Borrowed(SCHEDULE_MISSING_MSG)),
            ),
            TryRunRevUpdateError::MetaMissing => RunSystemError::Skipped(
                SystemParamValidationError::skipped::<RevMeta>(Cow::Borrowed(META_MISSING_MSG)),
            ),
            TryRunRevUpdateError::ScheduleAndMetaMissing => {
                RunSystemError::Skipped(SystemParamValidationError::skipped::<RevMeta>(
                    Cow::Borrowed(SCHEDULE_AND_META_MISSING_MSG),
                ))
            }
            TryRunRevUpdateError::AlreadyRunning { .. }
            | TryRunRevUpdateError::UndoRedoBufferNotEmptyBeforeUpdate { .. } => {
                RunSystemError::Skipped(SystemParamValidationError::invalid::<RevMeta>(format!(
                    "{self}"
                )))
            }
            TryRunRevUpdateError::AfterRunErrors { .. } => {
                RunSystemError::Failed(format!("{self}").into())
            }
        }
    }
}

const SCHEDULE_MISSING_MSG: &str =
    "schedule RevUpdate does not exist, it will not be run until it is inserted";
const META_MISSING_MSG: &str =
    "resource RevMeta does not exist, schedule RevUpdate will not be run until it is inserted";
const SCHEDULE_AND_META_MISSING_MSG: &str = "schedule RevUpdate and resource RevMeta do not exist, the schedule will not be run until both are inserted";

impl<M: Debug, B: Debug> Display for TryRunRevUpdateError<M, B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // todo: write! -> writeln!
        match self {
            Self::ScheduleMissing => write!(f, "{SCHEDULE_MISSING_MSG}"),
            Self::MetaMissing => write!(f, "{META_MISSING_MSG}"),
            Self::ScheduleAndMetaMissing => write!(f, "{SCHEDULE_AND_META_MISSING_MSG}"),
            Self::UndoRedoBufferNotEmptyBeforeUpdate { meta, buffer } => write!(
                f,
                "the resource containing buffered UndoRedo implementors was not empty, it contained the following types:\n{buffer:?}\n{meta:?}"
            ),
            Self::AlreadyRunning { meta, direction } => {
                write!(f, "RevMeta is already running at {direction:?}\n{meta:?}")
            }
            Self::AfterRunErrors {
                meta,
                update_logs_missed,
                despawn_cleaner_err,
            } => {
                if matches!(
                    *despawn_cleaner_err,
                    Some(DespawnCleanerErr::MetaNotRunning)
                ) {
                    write!(f, "RevMeta stopped running early\n")?;
                }
                if !update_logs_missed.is_empty() {
                    write!(
                        f,
                        "UpdateLog instances did not run when they were expected to:\n{update_logs_missed:?}\n"
                    )?;
                }
                if matches!(
                    *despawn_cleaner_err,
                    Some(DespawnCleanerErr::CleanerOutOfLog)
                ) {
                    write!(
                        f,
                        "the resource that finally despawns entities that were reversibly marked for despawn unexpectedly went out-of-log\n"
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

impl<M: Debug, B: Debug> Error for TryRunRevUpdateError<M, B> {}
