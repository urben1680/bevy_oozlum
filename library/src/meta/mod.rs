use core::{
    fmt::{Debug, Display},
    num::NonZeroU64,
    panic::Location,
};
use std::borrow::Cow;

use bevy_ecs::{
    change_detection::{MaybeLocation, Tick},
    component::ComponentId,
    error::Result as BevyResult,
    query::FilteredAccessSet,
    resource::Resource,
    system::{
        Command, ReadOnlySystemParam, RunSystemError, SystemMeta, SystemParam,
        SystemParamValidationError,
    },
    world::{World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_log::info;

use crate::{
    log::{PreUpdateKind, UpdateLogLimits, UpdateLogMissed, UpdateLogState},
    prelude::RevUpdate,
    undo_redo::{DespawnFinalizerErr, UndoRedoBuffer, finalize_despawns},
};

#[cfg(test)]
mod test;

/// The central resource that runs [`RevUpdate`] with a controlled [`RevDirection`] via the
/// [`run_rev_update`] system. It also tracks the global log which contains the world states that
/// can be reversed/advanced to.
///
/// [`run_rev_update`]: Self::run_rev_update
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

impl RevMeta {
    pub(crate) const DEFAULT_MAX_PAST_LEN: u64 = 1;
    pub(crate) const DEFAULT_PAUSED: bool = false;

    /// Construct a new value.
    ///
    /// - `max_past_len` defines how many frames can be reverted to via [`set_queue`] with
    ///   [`RevQueue::RunBackwardLog`]. The amount can be later changed via
    ///   [`set_max_past_len`]. If `0` is used, the value is replaced with `1`.
    /// - `paused` defines if after inserting this `RevMeta` it will be attempted to run
    ///   [`RevUpdate`] right away. For this that schedule and the [`run_rev_update`] system
    ///   must have been added to the app. If it is inserted in the paused state, it can be unpaused
    ///   via [`set_queue`] with [`RevQueue::RunForward`].
    ///
    /// [`set_queue`]: Self::set_queue
    /// [`set_max_past_len`]: Self::set_max_past_len
    /// [`run_rev_update`]: Self::run_rev_update
    pub fn new(max_past_len: u64, paused: bool) -> Self {
        Self {
            past_end: 0,
            now: 0,
            future_end: 0,
            max_past_len: NonZeroU64::new(max_past_len).unwrap_or(NonZeroU64::MIN),
            direction: RunningOrRan::Pause { after_log: false },
            queue: (!paused).then_some(RevQueue::RunForward),
            log_exits: 0,
            log_clears: 0,
            update_log_limits: UpdateLogLimits::default(),
        }
    }

    /// Change the current [`RevQueue`] that will be applied right before the next time
    /// [`run_rev_update`] runs. See the `RevQueue` docs for more information.
    ///
    /// [`run_rev_update`]: Self::run_rev_update
    pub fn set_queue(&mut self, queue: RevQueue) {
        self.queue = Some(queue);
    }

    /// Remove the current [`RevQueue`] before it could be applied right before the next time
    /// [`run_rev_update`] runs.
    ///
    /// [`run_rev_update`]: Self::run_rev_update
    pub fn unset_queue(&mut self) {
        self.queue = None;
    }

    /// Get the current [`RevQueue`] that will be applied right before the next time
    /// [`run_rev_update`] runs.
    ///
    /// [`run_rev_update`]: Self::run_rev_update
    pub fn get_queue(&self) -> Option<RevQueue> {
        self.queue
    }

    /// Set how many past frames can be reveres to at most. If `0` is used, the value is replaced
    /// with `1`.
    ///
    /// Note that this is coming into effect right before the next time [`run_rev_update`] runs. If
    /// at that point one or more frames of the log fall past that limit, the log will be truncated.
    /// This is final, increasing the limit again will not bring back truncated log entries.
    ///
    /// [`run_rev_update`]: Self::run_rev_update
    pub fn set_max_past_len(&mut self, max_past_len: u64) {
        self.max_past_len = NonZeroU64::new(max_past_len).unwrap_or(NonZeroU64::MIN);
    }

    /// Get how many past frames can be reveres to at most.
    pub fn get_max_past_len(&self) -> NonZeroU64 {
        self.max_past_len
    }

    /// Get at which direction [`RevUpdate`] is currently running.
    ///
    /// # Panics
    ///
    /// This method panics when `RevUpdate` is not currently running. Use [`get_running_direction`]
    /// for a fallible version.
    ///
    /// [`get_running_direction`]: Self::get_running_direction
    pub fn running_direction(&self) -> RevDirection {
        self.get_running_direction().unwrap()
    }

    /// Get at which direction [`RevUpdate`] is currently running.
    ///
    /// Returns `None` if `RevUpdate` is not currently running.
    pub fn get_running_direction(&self) -> Option<RevDirection> {
        match self.direction {
            RunningOrRan::Running(direction) => Some(direction),
            _ => None,
        }
    }

    /// Get at which direction [`RevUpdate`] was running the last time.
    ///
    /// Returns `None` if `RevUpdate` is currently running or `RevMeta` is currently [paused].
    ///
    /// [paused]: Self::paused
    pub fn get_ran_direction(&self) -> Option<RevDirection> {
        match self.direction {
            RunningOrRan::Ran(direction) => Some(direction),
            _ => None,
        }
    }

    /// Returns `true` if `RevMeta` is currently paused. Returns `false` otherwise.
    pub fn paused(&self) -> bool {
        matches!(self.direction, RunningOrRan::Pause { .. })
    }

    /// The reversible frame of the [`RevUpdate`] schedule.
    ///
    /// When the schedule is not running, this frame can be understood as the id of the present
    /// world state. This is not increased or decreased multiple times per reversible frame, so it
    /// is **no** reversible alternative to [`Tick`].
    ///
    /// When the schedule is running, the world state of this id reflects what the systems should
    /// work towards to. For example when the schedule is running [forward], `now` is increased from
    /// `n` to `n+1` and the reversible systems now have to bring the world state to from `n` to
    /// `n+1`.
    ///
    /// That is why, when the schedule is running backward, the returned value is not the same but
    /// one less than when it ran forward just previously, as the reversible systems have to bring
    /// the world from `n+1` back to `n` again.
    ///
    /// Can only be `0` at or after [`RevDirection::BackwardLog`] and is otherwise non-zero.
    ///
    /// [`Tick`]: bevy_ecs::change_detection::Tick
    /// [forward]: RevDirection::is_forward
    pub const fn now(&self) -> u64 {
        self.now
    }

    /// Returns the most past frame that currently can be reversed to.
    pub fn past_end(&self) -> u64 {
        self.past_end
    }

    /// Returns the most future frame that currently can be advanced to.
    ///
    /// During [`RevDirection::NotLog`], the [current] frame is also the future end.
    ///
    /// [current]: Self::now
    pub fn future_end(&self) -> u64 {
        self.future_end
    }

    /// Returns how many frames can be reversed by.
    pub fn past_len(&self) -> u64 {
        self.now - self.past_end
    }

    /// Returns the current [`NotLog`].
    ///
    /// # Panics
    ///
    /// This method panics when [`RevUpdate`] is not currently running at [`RevDirection::NotLog`].
    /// Use [`get_not_log`] for a fallible version.
    ///
    /// [`get_not_log`]: Self::get_not_log
    pub fn not_log(&self) -> NotLog {
        self.get_not_log().unwrap()
    }

    /// Returns the current [`NotLog`].
    ///
    /// Returns `None` if [`RevUpdate`] is not currently running at [`RevDirection::NotLog`].
    pub fn get_not_log(&self) -> Option<NotLog> {
        match self.direction {
            RunningOrRan::Running(RevDirection::NotLog(not_log)) => Some(not_log),
            _ => None,
        }
    }

    /// Returns how many frames can be advanced by.
    pub fn future_len(&self) -> u64 {
        self.future_end - self.now
    }

    /// Returns the total amount of frames in the log. This is the sum of [`past_len`] and
    /// [`future_len`] plus 1 to account for the current frame.
    ///
    /// [`past_len`]: Self::past_len
    /// [`future_len`]: Self::future_len
    pub fn len(&self) -> u64 {
        self.future_end - self.past_end + 1 // both ends are inclusive
    }

    /// Returns the total amount of times the [`running_direction`] changed from a [log direction]
    /// to [`RevDirection::NotLog`] since this `RevMeta` was constructed.
    ///
    /// [`running_direction`]: Self::running_direction
    /// [log direction]: RevDirection::is_log
    pub fn log_exits(&self) -> u64 {
        self.log_exits
    }

    /// Returns the total amount of times [`RevQueue::ClearThenRunForward`] or
    /// [`RevQueue::ClearThenPause`] were applied since this `RevMeta` was constructed.
    pub fn log_clears(&self) -> u64 {
        self.log_clears
    }

    /// Returns `true` if `frame` is in `[past_end, future_end]`.
    pub fn contains(&self, frame: u64) -> bool {
        self.future_end.wrapping_sub(frame) <= (self.future_end - self.past_end)
    }

    /// Returns `true` if `frame` is in `[past_end, now[`.
    pub fn past_contains(&self, frame: u64) -> bool {
        self.now.wrapping_sub(frame).wrapping_sub(1) < self.past_len()
    }

    /// Returns `true` if `frame` is in `]now, future_end]`.
    pub fn future_contains(&self, frame: u64) -> bool {
        self.future_end.wrapping_sub(frame) < self.future_len()
    }

    /// Returns `true` if [`now`] is equal to [`past_end`].
    ///
    /// [`now`]: Self::now
    /// [`past_end`]: Self::past_end
    pub fn end_of_log_backward(&self) -> bool {
        self.now == self.past_end
    }

    /// Returns `true` if [`now`] is equal to [`future_end`].
    ///
    /// [`now`]: Self::now
    /// [`future_end`]: Self::future_end
    pub fn end_of_log_forward(&self) -> bool {
        self.now == self.future_end
    }

    /// Update `RevMeta` and run `c` once unless paused. `c` should return it's `RevMeta` argument
    /// without replacing it at some point. It may be mutated however with any method **except**
    /// this one.
    ///
    /// This is used in [`run_rev_update`] and should not be called manually unless the mentioned
    /// system is replaced with something custom. In that case [`finalize_despawns`] should be used
    /// in the closure while `RevMeta` was inserted into the world.
    ///
    /// # Errors
    ///
    /// - If this method is called recursively or `RevMeta` was removed while this ran and is still
    ///   [in a running state] while this method is called again, this will return
    ///   [`RevMetaUpdateErr::AlreadyRunning`].
    /// - If `c` does not return `RevMeta`, this will return
    ///   [`RevMetaUpdateErr::RevMetaNotReturned`].
    /// - If `RevMeta` was replaced in `c` with a value that is not running in the same direction,
    ///   this will return [`RevMetaUpdateErr::DirectionChanged`].
    /// - If any [`UpdateLog`] did not update when it was expected to, in the amount it was expected
    ///   to, this will return [`RevMetaUpdateErr::UpdateLogsMissed`]. This may only happen during
    ///   [log directions].
    ///
    /// [`run_rev_update`]: Self::run_rev_update
    /// [in a running state]: Self::running_direction
    /// [`UpdateLog`]: crate::log::UpdateLog
    /// [log directions]: RevDirection::is_log
    pub fn update(
        mut self,
        c: impl FnOnce(Self, RevDirection) -> Option<Self>,
    ) -> Result<Self, RevMetaUpdateErr> {
        // get direction that ran previously
        let (ran, after_log) = match self.direction {
            RunningOrRan::Ran(RevDirection::NotLog(_)) => (
                Some(RevDirection::NOT_LOG_MIN), // gets updated below
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
            RunningOrRan::Running(_) => {
                return Err(RevMetaUpdateErr::AlreadyRunning { meta: self });
            }
        };

        // get queued direction
        let queue = match self.queue.take() {
            None => None,
            Some(RevQueue::RunForward) => {
                if after_log {
                    self.log_exits += 1;
                }
                Some(RevDirection::NOT_LOG_MIN) // gets updated below
            }
            Some(RevQueue::RunForwardLog) if !self.end_of_log_forward() => {
                Some(RevDirection::ForwardLog)
            }
            Some(RevQueue::RunBackwardLog) if !self.end_of_log_backward() => {
                Some(RevDirection::BackwardLog)
            }
            Some(RevQueue::ClearThenRunForward) => {
                self.clear();
                Some(RevDirection::NOT_LOG_MIN) // gets updated below
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
            RevDirection::NotLog(_) => {
                self.now += 1;
                self.future_end = self.now;
                let max_past_len = self.max_past_len.get();
                if self.past_len() > max_past_len {
                    self.past_end = self.now - max_past_len;
                }
                let past_len = NonZeroU64::new(self.now - self.past_end)
                    .expect("now is increased here and larger than past_end");
                RevDirection::NotLog(NotLog(past_len))
            }
            RevDirection::ForwardLog => {
                self.now += 1;
                RevDirection::ForwardLog
            }
            RevDirection::BackwardLog => {
                // todo: consider to do this after `c` ran
                self.now -= 1;
                RevDirection::BackwardLog
            }
        };

        // set running direction, call closure, set ran direction
        self.direction = RunningOrRan::Running(direction);
        let immutable_running_state = self.immutable_running_state();
        let Some(mut meta) = c(self, direction) else {
            return Err(RevMetaUpdateErr::RevMetaNotReturned);
        };
        if meta.immutable_running_state() != immutable_running_state {
            return Err(RevMetaUpdateErr::RevMetaReplaced { meta });
        }
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

    /// Clears the global log.
    fn clear(&mut self) {
        self.past_end = self.now;
        self.future_end = self.now;
        self.log_clears = self.log_clears.checked_add(1).unwrap(); // overflow not supported
        self.log_exits = 0;
        self.update_log_limits.clear();
        info!(
            "`RevQueue::Clear` was applied, `RevMeta::log_clears` is now {}, all `UpdateLog::id` \
            until now are invalid and will be reinitialized at their next mutation",
            self.log_clears
        )
    }

    /// Values that should not change while [running].
    ///
    /// [running]: RevMeta::running_direction
    fn immutable_running_state(&self) -> impl PartialEq + 'static {
        (
            self.past_end,
            self.now,
            self.future_end,
            self.direction,
            self.log_exits,
            self.log_clears,
        )
    }

    #[cfg(test)]
    pub(crate) fn update_ref(
        &mut self,
        should_run: Result<bool, UpdateLogMissed>,
        c: impl FnOnce(&mut Self, RevDirection),
    ) {
        let meta = core::mem::replace(self, RevMeta::new(u64::MAX, true));
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

    /// See [`UpdateLogLimits::set_update_state`].
    pub(super) fn set_update_state(
        &self,
        state: &mut Option<UpdateLogState>,
        caller: MaybeLocation<Option<&'static Location>>,
    ) -> PreUpdateKind {
        self.update_log_limits
            .set_update_state(state, self.log_exits, self.log_clears, caller)
    }

    /// Gets [`UpdateLogLimits`].
    pub(super) fn update_log_limits(&self) -> &UpdateLogLimits {
        &self.update_log_limits
    }
}

/// Update [`RevMeta`] and run [`RevUpdate`] once unless paused.
///
/// This can fail if `RevMeta` or internal resources are removed or replaced. Otherwise, the
/// only common source of error is doing mistakes at updating [`UpdateLog`]s at the expected
/// frames in the expected amounts.
///
/// [`UpdateLog`]: crate::log::UpdateLog
pub fn run_rev_update(world: &mut World) -> Result<(), RunSystemError> {
    world
        .try_schedule_scope(RevUpdate, |world, schedule| {
            // check for skipping conditions
            let Some(meta) = world.remove_resource::<RevMeta>() else {
                return Err(RunSystemError::Skipped(
                    SystemParamValidationError::skipped::<RevMeta>(Cow::Borrowed(
                        "resource RevMeta does not exist, schedule RevUpdate will not be run \
                    until it is inserted",
                    )),
                ));
            };

            if let Some(buffer) = world.get_resource::<UndoRedoBuffer>() {
                if !buffer.is_empty() {
                    let err = Err(RunSystemError::Skipped(
                        SystemParamValidationError::invalid::<RevMeta>(format!(
                            "the resource containing buffered UndoRedo implementors was not \
                        empty, it contained the following types:\n{buffer:?}\n{meta:?}"
                        )),
                    ));
                    world.insert_resource(meta);
                    return err;
                }
            }

            // update RevMeta and DespawnFinalizer
            let mut despawn_finalizer_result = Ok(());
            let meta_result = meta.update(|meta, _| {
                world.insert_resource(meta);
                schedule.run(world);
                despawn_finalizer_result = finalize_despawns(world);
                world.remove_resource::<RevMeta>()
            });

            // map errors
            match meta_result {
                Ok(meta) => {
                    let Err(err) = despawn_finalizer_result else {
                        world.insert_resource(meta);
                        return Ok(());
                    };
                    let err = Err(RunSystemError::Failed(
                        match err {
                            DespawnFinalizerErr::OutOfLog => format!(
                                "the resource that finally despawns entities that were reversibly \
                            marked for spawn or despawn went out-of-log\n{meta:?}"
                            ),
                            DespawnFinalizerErr::MetaNotRunning => format!(
                                "RevMeta stopped running early, it may have been replaced\n{meta:?}"
                            ),
                            DespawnFinalizerErr::MetaMissing => unreachable!(
                                "update_spawn_despawn would skip all logic without RevMeta, \
                            nothing could return it to be present again here"
                            ),
                        }
                        .into(),
                    ));
                    world.insert_resource(meta);
                    err
                }
                Err(RevMetaUpdateErr::AlreadyRunning { meta }) => {
                    let err = Err(RunSystemError::Skipped(
                        SystemParamValidationError::invalid::<RevMeta>(format!(
                            "RevMeta is already running\n{meta:?}"
                        )),
                    ));
                    world.insert_resource(meta);
                    err
                }
                Err(RevMetaUpdateErr::RevMetaNotReturned) => {
                    Err(RunSystemError::Failed(match despawn_finalizer_result {
                        Ok(()) => "RevMeta was removed during RevUpdate, possible in hooks or \
                            observers related to despawns"
                            .into(),
                        Err(DespawnFinalizerErr::MetaMissing) => "RevMeta was removed during \
                            RevUpdate"
                            .into(),
                        Err(DespawnFinalizerErr::OutOfLog)
                        | Err(DespawnFinalizerErr::MetaNotRunning) => unreachable!(
                            "when update_spawn_despawn returns {despawn_finalizer_result:?}, \
                            then only when RevMeta existed at that point, but then nothing is \
                            executed that could have removed RevMeta here"
                        ),
                    }))
                }
                Err(RevMetaUpdateErr::RevMetaReplaced { meta }) => {
                    let err = Err(RunSystemError::Failed(
                        format!("RevMeta was replaced with a different value\n{meta:?}").into(),
                    ));
                    world.insert_resource(meta);
                    err
                }
                Err(RevMetaUpdateErr::UpdateLogsMissed {
                    meta,
                    update_logs_missed,
                }) => {
                    // todo: use fmt::from_fn instead of format! when bevy switches to 1.93
                    let err = format!(
                        "UpdateLog instances did not run when they were expected \
                        to:\n{update_logs_missed:?}\n{meta:?}"
                    );

                    world.insert_resource(meta);

                    Err(RunSystemError::Failed(
                        match despawn_finalizer_result {
                            Ok(()) => format!("{err}"),
                            Err(DespawnFinalizerErr::OutOfLog) => format!(
                                "the resource that finally despawns entities that were reversibly \
                            marked for spawn or despawn went out-of-log, additionally {err:?}"
                            ),
                            Err(DespawnFinalizerErr::MetaNotRunning) => format!(
                                "RevMeta stopped running early, it may have been replaced, \
                            additionally {err:?}"
                            ),
                            Err(DespawnFinalizerErr::MetaMissing) => unreachable!(
                                "update_spawn_despawn would skip all logic without RevMeta, \
                            nothing could return it to be present again here"
                            ),
                        }
                        .into(),
                    ))
                }
            }
        })
        .unwrap_or_else(|_| {
            let err = if world.contains_resource::<RevMeta>() {
                "schedule RevUpdate does not exist, it will not be run until it is inserted"
            } else {
                "schedule RevUpdate and resource RevMeta do not exist, the schedule will not \
                be run until both are inserted"
            };
            Err(RunSystemError::Skipped(
                SystemParamValidationError::skipped::<RevMeta>(Cow::Borrowed(err)),
            ))
        })
}

/// The direction [`RevUpdate`] is currently running at. Reversible systems should mind this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub enum RevDirection {
    /// The world is updated for a new reversible frame. If [this particular frame] or
    /// [any future frame] existed in the log, they will be truncated and replaced from now on.
    ///
    /// [this particular frame]: RevMeta::now
    /// [any future frame]: RevMeta::future_len
    NotLog(NotLog),

    /// The world is advanced in the log.
    ForwardLog,

    /// The world is reversed in the log.
    BackwardLog,
}

impl RevDirection {
    pub(crate) const NOT_LOG_MIN: Self = Self::NotLog(NotLog(NonZeroU64::MIN));

    /// Is [NotLog] or [`ForwardLog`].
    ///
    /// [NotLog]: Self::NotLog
    /// [`ForwardLog`]: Self::ForwardLog
    pub fn is_forward(self) -> bool {
        !self.is_backward()
    }

    /// Is [`BackwardLog`].
    ///
    /// [`BackwardLog`]: Self::BackwardLog
    pub fn is_backward(self) -> bool {
        matches!(self, Self::BackwardLog)
    }

    /// Is [`ForwardLog`] or [`BackwardLog`].
    ///
    /// [`ForwardLog`]: Self::ForwardLog
    /// [`BackwardLog`]: Self::BackwardLog
    pub fn is_log(self) -> bool {
        !self.is_not_log()
    }

    /// Is [NotLog].
    ///
    /// [NotLog]: Self::NotLog
    pub fn is_not_log(self) -> bool {
        matches!(self, Self::NotLog(_))
    }

    /// Returns `NotLog` in [NotLog].
    ///
    /// # Panics
    ///
    /// This method panics for different directions.
    ///
    /// [NotLog]: Self::NotLog
    pub fn past_len(self) -> NotLog {
        self.get_past_len().unwrap()
    }

    /// Returns `NotLog` in [NotLog].
    ///
    /// Returns `None` for different directions.
    ///
    /// [NotLog]: Self::NotLog
    pub fn get_past_len(self) -> Option<NotLog> {
        match self {
            Self::NotLog(not_log) => Some(not_log),
            _ => None,
        }
    }
}

impl Display for RevDirection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            RevDirection::NotLog(_) => write!(f, "RevDirection::NotLog"),
            RevDirection::ForwardLog => write!(f, "RevDirection::ForwardLog"),
            RevDirection::BackwardLog => write!(f, "RevDirection::BackwardLog"),
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

/// The next state [`RevMeta`] should be in via [`RevMeta::set_queue`], will be applied when
/// [`run_rev_update`] runs. Before that, a different queue can be set, which will
/// overwrite a different pending value. Can also be [unset] before that.
///
/// [unset]: RevMeta::unset_queue
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub enum RevQueue {
    /// Run in [`RevDirection::NotLog`] next.
    ///
    /// If there is a [future segment], it will be truncated globally.
    ///
    /// If the [past segment] is longer than [the maximum], the excessive past end will be truncated
    /// globally.
    ///
    /// [future segment]: RevMeta::future_len
    /// [past segment]: RevMeta::past_len
    /// [the maximum]: RevMeta::get_max_past_len
    RunForward,

    /// Run in [`RevDirection::ForwardLog`] next until the [future end] is reached, then `RevMeta`
    /// will be paused. If it is already at that, this will pause directly.
    ///
    /// [future end]: RevMeta::future_end
    RunForwardLog,

    /// Run in [`RevDirection::BackwardLog`] next until the [past end] is reached, then `RevMeta`
    /// will be paused. If it is already at that, this will pause directly.
    ///
    /// [past end]: RevMeta::past_end
    RunBackwardLog,

    /// Pause `RevMeta` until a different queue will be set.
    Pause,

    /// Globally truncate the full [log], then run [`RevDirection::NotLog`] next.
    ///
    /// [log]: RevMeta::len
    ClearThenRunForward,

    /// Globally truncate the full [log], then pause `RevMeta`.
    ///
    /// [log]: RevMeta::len
    ClearThenPause,
}

impl Command<BevyResult> for RevQueue {
    fn apply(self, world: &mut World) -> BevyResult {
        world
            .get_resource_mut::<RevMeta>()
            .ok_or_else(|| format!("could not queue {self:?}, RevMeta is missing"))?
            .set_queue(self);
        Ok(())
    }
}

/// A newtyped value of [`RevMeta::past_len`] that only exists during [`RevDirection::NotLog`].
/// At that it can never be zero. It is used as a token to "prove" that particular direction is
/// running. Because of this, it should not be stored beyond a frame.
///
/// The [`RevCommands`]/[`RevEntityCommands`] including [`BuffersUndoRedo`] APIs need this as they
/// also should only be used during that direction.
///
/// [`TransitionLog::forward_push`]/[`TransitionsLog::forward_extend`] also need this if they are
/// updated exactly once per reversible frame. If not, they should use the value returned by
/// [`UpdateLog::forward_past_len`] instead.
///
/// [`RevCommands`]: crate::undo_redo::RevCommands
/// [`RevEntityCommands`]: crate::undo_redo::RevEntityCommands
/// [`BuffersUndoRedo`]: crate::undo_redo::BuffersUndoRedo
/// [`TransitionLog::forward_push`]: crate::log::TransitionLog::forward_push
/// [`TransitionsLog::forward_extend`]: crate::log::TransitionsLog::forward_extend
/// [`UpdateLog::forward_past_len`]: crate::log::UpdateLog::forward_past_len`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub struct NotLog(NonZeroU64);

impl Into<NonZeroU64> for NotLog {
    fn into(self) -> NonZeroU64 {
        self.0
    }
}

unsafe impl SystemParam for NotLog {
    type State = ComponentId;
    type Item<'w, 's> = NotLog;

    fn init_state(world: &mut World) -> Self::State {
        world
            .components_registrator()
            .register_resource::<RevMeta>()
    }

    fn init_access(
        &component_id: &Self::State,
        system_meta: &mut SystemMeta,
        component_access_set: &mut FilteredAccessSet,
        _world: &mut World,
    ) {
        let combined_access = component_access_set.combined_access();
        assert!(
            !combined_access.has_resource_write(component_id),
            "error[B0002]: NotLog in system {} conflicts with a previous ResMut<RevMeta> access. Consider removing the duplicate access. See: https://bevy.org/learn/errors/b0002",
            system_meta.name(),
        );

        component_access_set.add_unfiltered_resource_read(component_id);
    }

    #[inline]
    unsafe fn validate_param(
        &mut component_id: &mut Self::State,
        _system_meta: &SystemMeta,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        // SAFETY: Read-only access to resource metadata.
        let meta = unsafe {
            world
                .get_resource_by_id(component_id)
                .map(|ptr| ptr.deref())
        };
        if meta.and_then(RevMeta::get_not_log).is_some() {
            Ok(())
        } else {
            Err(SystemParamValidationError::skipped::<Self>(
                "RevMeta does not exist or RevUpdate is not running or is running in log",
            ))
        }
    }

    #[inline]
    unsafe fn get_param<'w, 's>(
        &mut component_id: &'s mut Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell<'w>,
        _change_tick: Tick,
    ) -> NotLog {
        // SAFETY: Read-only access to resource metadata.
        let meta = unsafe {
            world
                .get_resource_by_id(component_id)
                .map(|ptr| ptr.deref())
        };
        meta.and_then(RevMeta::get_not_log).unwrap_or_else(|| {
            panic!(
                "Resource requested by {} does not exist: RevMeta",
                system_meta.name()
            );
        })
    }
}

// SAFETY: NotLog only reads RevMeta resource
unsafe impl ReadOnlySystemParam for NotLog {}

/// Error type that [`RevMeta::update`] may return.
#[derive(Debug)]
pub enum RevMetaUpdateErr {
    /// [`RevMeta::update`] was called recursively or `RevMeta` was removed while this ran and is
    /// still [in a running state] while the `update` method is called again.
    ///
    /// [in a running state]: RevMeta::running_direction
    AlreadyRunning {
        /// `RevMeta` in the state it was attempted to be updated with.
        meta: RevMeta,
    },

    /// The closure of [`RevMeta::update`] did not return `RevMeta`.
    RevMetaNotReturned,

    /// `RevMeta` was replaced with a different value during [`RevMeta::update`].
    RevMetaReplaced {
        /// `RevMeta` in the state as it was returned from the closure of [`RevMeta::update`].
        meta: RevMeta,
    },

    /// Any [`UpdateLog`] did not update when it was expected to, in the amount it was expected
    /// to. This may only happen during [log directions].
    ///
    /// [`UpdateLog`]: crate::log::UpdateLog
    /// [log directions]: RevDirection::is_log
    UpdateLogsMissed {
        /// `RevMeta` in the state after it was updated regardless of this error.
        meta: RevMeta,

        /// Information about which [`UpdateLog`]s did not update as they should have.
        ///
        /// [`UpdateLog`]: crate::log::UpdateLog
        update_logs_missed: Vec<UpdateLogMissed>,
    },
}
