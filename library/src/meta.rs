use core::num::NonZeroUsize;
use std::ops::RangeInclusive;

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::ComponentId,
        event::Event,
        query::Access,
        schedule::{InternedScheduleLabel, ScheduleLabel},
        system::{IntoSystem, Res, Resource, System},
        world::World,
    },
    log::warn_once,
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::info,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::{
    log::{PackedTime, WithLoggedAt},
    world::RevWorld,
    RevUpdate,
};

mod verifying;

pub use verifying::{VerifyError, VerifyingRevMeta};

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
                assert!(range.start() + updates_until_pause.get() - 1 <= now, "{meta:?}");
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
