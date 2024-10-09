use core::num::NonZeroUsize;
use std::ops::{Deref, RangeInclusive};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::ComponentId,
        event::Event,
        query::Access,
        schedule::{InternedScheduleLabel, ScheduleLabel},
        system::{IntoSystem, ReadOnlySystemParam, Res, Resource, System, SystemMeta, SystemParam},
        world::World,
    },
    log::{error, warn_once},
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::info,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::{
    log::{OutOfLog, PackedTime, StateLog, WithLoggedAt},
    world::RevWorld,
    RevUpdate,
};

#[derive(Clone, Debug)]
pub enum RevTryRunScheduleError {
    RevMetaMissing {
        existed_previously: bool,
        first_call: bool,
    },
    NoRevScheduleRunning {
        meta: RevMeta,
    },
    ScheduleMissing {
        meta: RevMeta,
        schedule: InternedScheduleLabel,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub enum RevDirection {
    Forward { log: bool },
    BackwardLog,
}

#[allow(non_upper_case_globals)] // every crate need a little crime
impl RevDirection {
    pub const NotLog: Self = Self::Forward { log: false };
    pub const ForwardLog: Self = Self::Forward { log: true };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub enum InternalDirection {
    RunningForward,
    RunningForwardLog { updates_until_pause: NonZeroUsize },
    RunningBackwardLog { updates_until_pause: NonZeroUsize },
    RanForward,
    RanForwardLog { updates_until_pause: NonZeroUsize },
    RanBackwardLog { updates_until_pause: NonZeroUsize },
    Pause,
}

impl InternalDirection {
    fn start_running(&mut self) {
        *self = match *self {
            Self::RanForward => Self::RunningForward,
            Self::RanForwardLog {
                updates_until_pause,
            } => Self::RunningForwardLog {
                updates_until_pause,
            },
            Self::RanBackwardLog {
                updates_until_pause,
            } => Self::RunningBackwardLog {
                updates_until_pause,
            },
            _ => *self,
        }
    }
    fn end_running(&mut self) {
        *self = match *self {
            Self::RunningForward => Self::RanForward,
            Self::RunningForwardLog {
                updates_until_pause,
            } => Self::RanForwardLog {
                updates_until_pause,
            },
            Self::RunningBackwardLog {
                updates_until_pause,
            } => Self::RanBackwardLog {
                updates_until_pause,
            },
            _ => *self,
        }
    }
    pub fn get_direction(self) -> Option<RevDirection> {
        match self {
            Self::RunningForward => Some(RevDirection::NotLog),
            Self::RunningForwardLog { .. } => Some(RevDirection::ForwardLog),
            Self::RunningBackwardLog { .. } => Some(RevDirection::BackwardLog),
            _ => None,
        }
    }
    pub fn running_rev_schedule(self) -> bool {
        matches!(
            self,
            Self::RunningForward | Self::RunningForwardLog { .. } | Self::RunningBackwardLog { .. }
        )
    }
}

#[derive(Clone, Copy, Debug, Event)]
pub struct ReduceLoggedAt(NonZeroUsize);

impl ReduceLoggedAt {
    pub fn by(self) -> usize {
        self.0.get()
    }
}

#[derive(Resource, Default)]
pub(crate) struct CommandsLogReducings(pub(crate) Vec<CommandsLogReducingBox>);

pub(crate) type CommandsLogReducingBox =
    Box<dyn Fn(ReduceLoggedAt, &mut World) + Send + Sync + 'static>;

/// RevMeta is used to control the processing of reversible systems.
///
/// It keepts track what the current frame is and to which frame one can go forward and backward in time.
#[derive(Debug, Clone, Resource, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct RevMeta {
    /// The maximum amount of states of the world that can be jumped to, or None if growth is unrestricted.
    ///
    /// As the world is always in a certain state, the amount cannot be zero.
    ///
    /// World states that become too old will no longer be accessible after an update, even if raising this value afterwards.
    /// If one wants to keep a certain `frame` accessible, one needs to _either_:
    /// - regularily set this value to not less than `now() + 2 - frame` before the next update
    /// - set it to `None`, disabling forgetting world states
    ///
    /// Reducing this value alone does not cause deallocations, this has to be done manually with each [log structs](crate::log) if desired.
    ///
    /// Changing this value is always possible but only comes into effect when updating the world during [`Direction::NotLog`].
    ///
    /// If the value exceeds [`PackedTime::MAX_USIZE`], this has no effect as the log gets reduced by frame wrapping before that, see (todo).
    pub max_len: Option<NonZeroUsize>,
    now: usize,
    range: RangeInclusive<usize>,
    queue: Option<InternalDirection>,
    direction: InternalDirection,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Some(NonZeroUsize::MIN), 0, false)
    }
}

impl RevMeta {
    pub const fn new(max_len: Option<NonZeroUsize>, now: usize, paused: bool) -> Self {
        if now > PackedTime::MAX_USIZE {
            panic!("now must not be larger than PackedTime::MAX_USIZE")
        }
        Self {
            max_len,
            now,
            range: now..=now,
            direction: match paused {
                true => InternalDirection::Pause,
                false => InternalDirection::RanForward,
            },
            queue: None,
        }
    }
    pub fn direction(&self) -> RevDirection {
        self.get_direction().expect("todo")
    }
    pub fn get_direction(&self) -> Option<RevDirection> {
        self.direction.get_direction()
    }
    pub fn internal_direction(&self) -> InternalDirection {
        self.direction
    }
    pub fn running_rev_schedule(&self) -> bool {
        self.direction.running_rev_schedule()
    }
    pub fn now(&self) -> usize {
        self.now
    }
    pub fn past_len(&self) -> usize {
        self.now - self.range.start()
    }
    pub fn start(&self) -> usize {
        *self.range.start()
    }
    pub fn end_inclusive(&self) -> usize {
        *self.range.end()
    }
    pub fn with_logged_at<T>(&self, value: T) -> WithLoggedAt<T> {
        WithLoggedAt {
            value,
            logged_at: PackedTime::from_internal(self.now),
        }
    }
    pub fn log_range(&self) -> RangeInclusive<usize> {
        self.range.clone()
    }
    pub fn reduce_range(&mut self, range: RangeInclusive<usize>) -> Result<(), ()> {
        if range.start() >= self.range.start()
            && range.end() <= self.range.end()
            && range.contains(&self.now)
        {
            self.range = range;
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn clear(&mut self) {
        self.range = self.now..=self.now;
    }
    /// Queue to go forward.
    ///
    /// Will cause logged future frames to be forgotten.
    pub fn queue_forward(&mut self) {
        self.queue = Some(InternalDirection::RunningForward);
    }
    pub fn queue_log(&mut self, to: usize) -> Result<usize, RangeInclusive<usize>> {
        if !self.log_range().contains(&to) {
            return Err(self.log_range());
        }
        let updates_until_pause = to.abs_diff(self.now);
        self.queue = Some(NonZeroUsize::new(updates_until_pause).map_or(
            InternalDirection::Pause,
            |updates_until_pause| match to > self.now {
                true => InternalDirection::RunningForwardLog {
                    updates_until_pause,
                },
                false => InternalDirection::RunningBackwardLog {
                    updates_until_pause,
                },
            },
        ));
        Ok(updates_until_pause)
    }
    pub fn queue_pause(&mut self) {
        self.queue = Some(InternalDirection::Pause);
    }
    pub fn end_running(&mut self) {
        self.direction.end_running();
    }
    pub(crate) fn get_from_world(world: &mut World) -> Result<&mut Self, RevTryRunScheduleError> {
        #[derive(Resource, Clone)]
        struct Existed(bool);

        if world.contains_resource::<Self>() {
            world.insert_resource(Existed(true));
            Ok(world.resource_mut::<Self>().into_inner())
        } else {
            let err = match world.get_resource::<Existed>().cloned() {
                None => RevTryRunScheduleError::RevMetaMissing {
                    existed_previously: false,
                    first_call: true,
                },
                Some(Existed(existed_previously)) => RevTryRunScheduleError::RevMetaMissing {
                    existed_previously,
                    first_call: false,
                },
            };
            world.insert_resource(Existed(false));
            Err(err)
        }
    }
    pub fn try_update_world(world: &mut World) -> Result<(), RevTryRunScheduleError> {
        let meta = Self::get_from_world(world)?;
        let previous = meta.clone();
        let reduce_logged_at = meta.update();
        let meta = meta.clone();

        match meta.get_direction() {
            Some(direction) => {
                let result = world
                    .rev_try_schedule_scope(RevUpdate, |world, schedule| {
                        if let Some(reduce_logged_at) = reduce_logged_at {
                            world.trigger(reduce_logged_at);
                            if let Some(reducings) = world.remove_resource::<CommandsLogReducings>()
                            {
                                for reducing in &reducings.0 {
                                    reducing(reduce_logged_at, world);
                                }
                                world.insert_resource(reducings);
                            }
                        }
                        match direction {
                            RevDirection::Forward { .. } => schedule.run_forward(world),
                            RevDirection::BackwardLog => schedule.run_backward(world),
                        }
                    })
                    .map_err(|_| RevTryRunScheduleError::ScheduleMissing {
                        meta,
                        schedule: RevUpdate.intern(),
                    });
                if let Some(mut meta) = world.get_resource_mut::<Self>() {
                    match result {
                        Ok(()) => meta.end_running(),
                        Err(_) => *meta = previous,
                    }
                }
                result
            }
            None => {
                if let Some(mut this) = world.get_resource_mut::<Self>() {
                    this.end_running()
                }
                Err(RevTryRunScheduleError::NoRevScheduleRunning { meta })
            }
        }
    }
    pub fn update_world(world: &mut World) {
        match Self::try_update_world(world) {
            Err(RevTryRunScheduleError::RevMetaMissing { first_call: true, .. }) => info!(
                "RevMeta does not exist yet, reversible schedule RevUpdate will not be called until it is inserted."
            ),
            Err(RevTryRunScheduleError::RevMetaMissing { existed_previously: true, .. }) => info!(
                "RevMeta was removed, reversible schedule RevUpdate will not be called until it is inserted again."
            ),
            Err(RevTryRunScheduleError::ScheduleMissing { meta, .. }) => warn_once!(
                "RevMeta cannot find reversible schedule RevUpdate, make sure to not \
                run it recursively or call RevMeta::update_world recursively.\n{meta:?}"
            ),
            _ => {}
        }
    }
    pub fn update(&mut self) -> Option<ReduceLoggedAt> {
        match self.queue.take() {
            Some(queue) => {
                self.direction = queue;
                match self.get_direction() {
                    Some(RevDirection::NotLog) => return self.update_forward(),
                    Some(RevDirection::ForwardLog) => self.now += 1,
                    Some(RevDirection::BackwardLog) => self.now -= 1,
                    None => {}
                }
            }
            None => {
                self.direction.start_running();
                match &mut self.direction {
                    InternalDirection::RunningForward => return self.update_forward(),
                    InternalDirection::RunningForwardLog {
                        updates_until_pause,
                    } => match reduction_successful(updates_until_pause) {
                        true => self.now += 1,
                        false => self.direction = InternalDirection::Pause,
                    },
                    InternalDirection::RunningBackwardLog {
                        updates_until_pause,
                    } => match reduction_successful(updates_until_pause) {
                        true => self.now -= 1,
                        false => self.direction = InternalDirection::Pause,
                    },
                    _ => {}
                }
            }
        }
        None
    }
    fn update_forward(&mut self) -> Option<ReduceLoggedAt> {
        if self.now == PackedTime::MAX_USIZE {
            let mut reduce = *self.range.start();
            reduce = reduce.max(1); // force reduction if needed, ensure non-zero
            if let Some(max_len) = self.max_len {
                reduce = reduce.max(self.now.saturating_sub(max_len.get() - 1))
            }
            self.now -= reduce;
            self.range = 0..=self.now;
            Some(ReduceLoggedAt(
                NonZeroUsize::new(reduce).expect("ensured non-zero"),
            ))
        } else {
            self.now += 1;
            let mut start = *self.range.start();
            if let Some(max_len) = self.max_len {
                start = start.max(self.now.saturating_sub(max_len.get() - 1));
            }
            self.range = start..=self.now;
            None
        }
    }
    pub(crate) fn add_read_if_no_write(
        world: &mut World,
        component_access: &mut Access<ComponentId>,
        archetype_component_access: &mut Access<ArchetypeComponentId>,
    ) {
        /// Not everything of the bevy API that is needed here to update archetype_component_access is public,
        /// so this is a rather complicated way to do it while trying to make it cheap after the first call.
        /// The benefit is that this is agnostic to implementation details of how impl SystemParam for Res works.
        #[derive(Resource)]
        struct RevMetaAccesses {
            component_access: Access<ComponentId>,
            archarchetype_component_access: Access<ArchetypeComponentId>,
        }

        let access = match world.get_resource::<RevMetaAccesses>() {
            Some(access) => access,
            None => {
                let mut system = IntoSystem::into_system(|_: Res<Self>| {});
                system.initialize(world);
                world.insert_resource(RevMetaAccesses {
                    component_access: system.component_access().clone(),
                    archarchetype_component_access: system.archetype_component_access().clone(),
                });
                world.resource::<RevMetaAccesses>()
            }
        };

        if access.component_access.is_compatible(&component_access) {
            component_access.extend(&access.component_access);
        }
        if access
            .archarchetype_component_access
            .is_compatible(&archetype_component_access)
        {
            archetype_component_access.extend(&access.archarchetype_component_access);
        }
    }
}

fn reduction_successful(updates_until_pause: &mut NonZeroUsize) -> bool {
    NonZeroUsize::new(updates_until_pause.get() - 1)
        .map(|reduced| *updates_until_pause = reduced)
        .is_some()
}

/// `RevMeta` system param wrapper to keep track of the system's running frames.
///
/// Can be used to automatically verify that the system is running at the right
/// frame and to get the frame number the system last ran, if any.
#[derive(Debug, Copy, Clone)]
pub struct VerifyingRevMeta<'w, 's> {
    meta: &'w RevMeta,
    last_run_or_err: Result<Option<NonZeroUsize>, VerifyError<'s>>,
}

impl Deref for VerifyingRevMeta<'_, '_> {
    type Target = RevMeta;
    fn deref(&self) -> &Self::Target {
        self.meta
    }
}

impl VerifyingRevMeta<'_, '_> {
    /// Get the frame the system last ran.
    ///
    /// Returns None if the system did not run in the past.
    ///
    /// Note that this is the chronical last run and therefore is always in the past.
    ///
    /// # Panics
    ///
    /// Panics if the update of the value failed or if the the current frame does not
    /// match with the frame that is logged by this SystemParam.
    ///
    /// [`Self::get_last_run`] is a fallible variant.
    pub fn last_run(&self) -> Option<usize> {
        self.get_last_run().unwrap_or_else(|err| panic!(
            "VerifyingRevMeta::last_run panicked: VerifyingRevMeta::get_param failed previously, see log\n{err:#?}"
        ))
    }

    /// Get the frame the system last ran.
    ///
    /// Returns None if the system did not run in the past.
    ///
    /// Note that this is the chronical last run and therefore is always in the past.
    ///
    /// Returns Err if the update of the value failed or if the the current frame does not
    /// match with the frame that is logged by this SystemParam.
    pub fn get_last_run(&self) -> Result<Option<usize>, VerifyError> {
        self.last_run_or_err
            .map(|last_run| last_run.map(NonZeroUsize::get))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VerifyError<'s> {
    pub frame_log_at_err: &'s StateLog<WithLoggedAt>,
    pub meta_at_err: &'s RevMeta,
}

#[doc(hidden)]
pub struct VerifyingRevMetaState {
    meta: ComponentId,
    frame_log: StateLog<WithLoggedAt>,
    meta_at_err: Option<RevMeta>,
}

impl VerifyingRevMetaState {
    fn get_param<'w, 's>(
        &'s mut self,
        meta: &'w RevMeta,
        system_name: &str,
    ) -> VerifyingRevMeta<'w, 's> {
        let mut last_run = None;
        if self.meta_at_err.is_none() {
            last_run = self.update_state_get_last_run(meta, system_name);
        }
        match self.meta_at_err.as_ref() {
            None => VerifyingRevMeta {
                meta,
                last_run_or_err: Ok(last_run),
            },
            Some(meta_at_err) => VerifyingRevMeta {
                meta,
                last_run_or_err: Err(VerifyError {
                    frame_log_at_err: &self.frame_log,
                    meta_at_err,
                }),
            },
        }
    }
    fn update_state_get_last_run(
        &mut self,
        meta: &RevMeta,
        system_name: &str,
    ) -> Option<NonZeroUsize> {
        let mut last_run = 0;
        match meta.get_direction() {
            Some(RevDirection::NotLog) => {
                last_run = self.frame_log.logged_at();
                self.frame_log.pop_past_by_timestamp(meta.start());
                self.frame_log.push_present(meta.now.into());
            }
            Some(RevDirection::ForwardLog) => {
                last_run = self.frame_log.logged_at();
                if self.frame_log.forward_log() == Err(OutOfLog) {
                    self.out_of_log("forward", meta, system_name);
                } else if self.frame_log.logged_at() != meta.now() {
                    self.mismatch("forward", meta, system_name);
                }
            }
            Some(RevDirection::BackwardLog) => {
                if self.frame_log.logged_at() - 1 != meta.now() {
                    self.mismatch("backward", meta, system_name);
                } else if self.frame_log.backward_log() == Err(OutOfLog) {
                    self.out_of_log("backward", meta, system_name);
                }
                last_run = self.frame_log.logged_at();
            }
            None => self.non_rev_schedule(meta, system_name),
        };
        NonZeroUsize::new(last_run)
    }
    const SUGGESTION: &'static str = ", check if the schedule this system is added to is actually a reversible \
        schedule by using `rev_` prefixed methods on the `App` and that the schedule and is correctly triggered";
    fn out_of_log(&mut self, direction: &str, meta: &RevMeta, system_name: &str) {
        error!(
            "VerifyingRevMeta::get_param failed: system \"{system_name}\" is out of log during {direction} log \
            schedule, at least once a run during another schedule was missed{}\n{meta:#?}\n{:#?}",
            Self::SUGGESTION, self.frame_log
        );
        self.meta_at_err = Some(meta.clone());
    }
    fn mismatch(&mut self, direction: &str, meta: &RevMeta, system_name: &str) {
        let mut expected = self.frame_log.logged_at();
        if direction == "backward" {
            expected -= 1;
        }
        let actual = meta.now();
        error!(
            "VerifyingRevMeta::get_param failed: system \"{system_name}\" is expected to run at frame {expected} \
            but ran at  frame {actual} during {direction} log schedule{}\n{meta:#?}\n{:#?}",
            Self::SUGGESTION, self.frame_log
        );
        self.meta_at_err = Some(meta.clone());
    }
    fn non_rev_schedule(&mut self, meta: &RevMeta, system_name: &str) {
        error!(
            "VerifyingRevMeta::get_param failed: run of system \"{system_name}\" happened during non-reversible \
            schedule{}\n{meta:#?}\n{:#?}",
            Self::SUGGESTION, self.frame_log
        );
        self.meta_at_err = Some(meta.clone());
    }
}

unsafe impl SystemParam for VerifyingRevMeta<'_, '_> {
    type Item<'world, 'state> = VerifyingRevMeta<'world, 'state>;
    type State = VerifyingRevMetaState;
    fn init_state(world: &mut World, system_meta: &mut SystemMeta) -> Self::State {
        let meta = Res::<RevMeta>::init_state(world, system_meta);

        // 0 is a special value here, during forward schedules the current frame is never 0, so if the
        // value passed to Self::Item is 0, this indicates that the system did not run in a past frame.
        // This works better than wrapping the log in an Option that becomes Some at the first run as
        // then undoing that call would be undistinguishable to an out-of-log error.
        let logged_at = WithLoggedAt::from(0);

        VerifyingRevMetaState {
            meta,
            frame_log: logged_at.into(),
            meta_at_err: None,
        }
    }
    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        system_meta: &SystemMeta,
        world: bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell<'world>,
        _change_tick: bevy::ecs::component::Tick,
    ) -> Self::Item<'world, 'state> {
        let meta: &RevMeta = world
            .get_resource_by_id(state.meta)
            .expect("todo, upcoming verify params feature")
            .deref(); //SAFETY: correct ComponentId from Res::<RevMeta>::init_state
        state.get_param(meta, system_meta.name())
    }
}

// SAFETY: Only reads RevMeta
unsafe impl ReadOnlySystemParam for VerifyingRevMeta<'_, '_> {}

#[cfg(test)]
mod test {
    use super::*;

    const ONE: NonZeroUsize = NonZeroUsize::MIN;
    const TWO: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(2) };
    const THREE: NonZeroUsize = unsafe { NonZeroUsize::new_unchecked(3) };

    /// Constructs [`RevMeta`] and asserts the values are valid
    fn arrange(
        max_len: Option<NonZeroUsize>,
        now: usize,
        range: RangeInclusive<usize>,
        direction: InternalDirection,
    ) -> RevMeta {
        let meta = RevMeta {
            max_len,
            now,
            range: range.clone(),
            direction,
            queue: None,
        };
        assert!(*range.start() <= now, "{meta:?}");
        match direction {
            InternalDirection::RunningForward => assert_eq!(now, range.end() + 2, "{meta:?}"),
            InternalDirection::RunningForwardLog {
                updates_until_pause,
            } => {
                assert!(now <= *range.end(), "{meta:?}");
                assert!(
                    now + updates_until_pause.get() - 1 <= *range.end(),
                    "{meta:?}"
                );
            }
            InternalDirection::RunningBackwardLog {
                updates_until_pause,
            } => {
                assert!(now <= *range.end(), "{meta:?}");
                assert!(range.start() + updates_until_pause.get() <= now, "{meta:?}");
            }
            InternalDirection::Pause => assert!(now <= *range.end(), "{meta:?}"),
            _ => unimplemented!(),
        }
        meta
    }

    #[test]
    fn log_forward_defaults_to_pause() {
        let mut meta = arrange(
            None,
            0,
            0..=1,
            InternalDirection::RunningForwardLog {
                updates_until_pause: TWO,
            },
        );

        meta.update();
        assert_eq!(
            meta.internal_direction(),
            InternalDirection::RunningForwardLog {
                updates_until_pause: ONE
            }
        );
        assert_eq!(meta.now(), 1);

        meta.update();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.now(), 1);
    }

    #[test]
    fn log_backward_defaults_to_pause() {
        let mut meta = arrange(
            None,
            1,
            0..=1,
            InternalDirection::RunningBackwardLog {
                updates_until_pause: TWO,
            },
        );

        meta.update();
        assert_eq!(
            meta.internal_direction(),
            InternalDirection::RunningBackwardLog {
                updates_until_pause: ONE
            }
        );
        assert_eq!(meta.now(), 0);

        meta.update();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.now(), 0);
    }

    #[test]
    fn start_grows_according_to_max_len() {
        let mut meta = RevMeta::new(Some(TWO), 0, false);

        meta.update();
        assert_eq!(meta.now(), 1);
        assert_eq!(meta.log_range(), 0..=1);

        meta.update();
        assert_eq!(meta.now(), 2);
        assert_eq!(meta.log_range(), 1..=2);
    }

    #[test]
    fn queue_log_to_out_of_range_fails() {
        let mut meta = arrange(None, 2, 1..=3, InternalDirection::Pause);

        assert_eq!(meta.queue_log(0), Err(1..=3));
        assert_eq!(meta.queue_log(4), Err(1..=3));
    }

    #[test]
    fn non_log_forward_truncates_future() {
        let mut meta = arrange(
            None,
            2,
            0..=2,
            InternalDirection::RunningBackwardLog {
                updates_until_pause: THREE,
            },
        );

        meta.update();
        assert_eq!(meta.now(), 1);
        assert_eq!(meta.log_range(), 0..=2);

        meta.update();
        assert_eq!(meta.now(), 0);
        assert_eq!(meta.log_range(), 0..=2);

        meta.queue_forward();
        meta.update();
        assert_eq!(meta.now(), 1);
        assert_eq!(meta.log_range(), 0..=1);
    }
}
