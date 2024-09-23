use core::{num::NonZeroUsize, ops::Range};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        component::ComponentId,
        query::Access,
        system::{Local, Resource},
        world::World,
    },
    log::warn_once,
    prelude::{IntoSystem, Res, System},
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::info,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::{
    log::{PackedTime, WithLoggedAt},
    BackwardSchedule, ForwardSchedule, RevUpdate,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub enum Direction {
    Forward { log: bool },
    BackwardLog,
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
    pub fn get_direction(self) -> Option<Direction> {
        match self {
            Self::RunningForward => Some(Direction::Forward { log: false }),
            Self::RunningForwardLog { .. } => Some(Direction::Forward { log: true }),
            Self::RunningBackwardLog { .. } => Some(Direction::BackwardLog),
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
    /// The maximum amount of states of the world that is logged to be jumped in, or None if growth is unrestricted.
    ///
    /// As the world is always in a certain state, the amount cannot be zero.
    ///
    /// World states that become too old will no longer be accessible after an update, even if raising this value afterwards.
    /// If one wants to keep a certain frame accessible, one needs to _either_:
    /// - regularily set this value to not less than `now() + 2 - frame` before the next update
    /// - set it to `None`, disabling forgetting world states
    ///
    /// Reducing this value alone does not cause deallocations, this has to be done manually with each [`crate::log`] struct if desired.
    ///
    /// Changing this value is always possible but only comes into effect when updating the world during [`Direction::Forward`].
    pub max_len: Option<NonZeroUsize>,
    now: usize,
    range: Range<usize>,
    queue: Option<InternalDirection>,
    direction: InternalDirection,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Some(NonZeroUsize::MIN), 0, false)
    }
}

impl RevMeta {
    pub const MAX_FRAME: usize = PackedTime::MAX_USIZE - 1;
    pub const fn new(max_len: Option<NonZeroUsize>, now: usize, paused: bool) -> Self {
        if now >= Self::MAX_FRAME {
            panic!("now must be less than RevMeta::MAX_FRAME")
        }
        Self {
            max_len,
            now,
            range: now..now + 1,
            direction: match paused {
                true => InternalDirection::Pause,
                false => InternalDirection::RanForward,
            },
            queue: None,
        }
    }
    pub fn direction(&self) -> Direction {
        self.get_direction().expect("todo")
    }
    pub fn get_direction(&self) -> Option<Direction> {
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
        self.now - self.range.start
    }
    pub fn with_timestamp<T>(&self, value: T) -> WithLoggedAt<T> {
        WithLoggedAt {
            value,
            logged_at: PackedTime::from_internal(self.now),
        }
    }
    /// Returns the frame range that can be returned to using [`Self::queue_log`].
    pub fn log_range(&self) -> Range<usize> {
        self.range.clone()
    }
    pub fn reduce_range(&mut self, range: Range<usize>) -> Result<(), ()> {
        if range.start >= self.range.start
            && range.end <= self.range.end
            && range.contains(&self.now)
        {
            self.range = range;
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn clear(&mut self) {
        self.range = self.now..self.now + 1;
    }
    /// Queue to go forward.
    ///
    /// Will cause logged future frames to be forgotten.
    pub fn queue_forward(&mut self) {
        self.queue = Some(InternalDirection::RunningForward);
    }
    pub fn queue_log(&mut self, to: usize) -> Result<usize, Range<usize>> {
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
    pub fn update_world(world: &mut World, mut rev_meta_exists: Local<Option<bool>>) {
        let Some(mut this) = world.get_resource_mut::<Self>() else {
            match *rev_meta_exists {
                None => info!("RevMeta does not exist yet, reversible schedule RevUpdate will not be called until it is inserted."),
                Some(true) => info!("RevMeta was removed, reversible schedule RevUpdate will not be called until it is inserted again."),
                _ => {}
            }
            *rev_meta_exists = Some(false);
            return;
        };
        *rev_meta_exists = Some(true);
        this.update();
        let this = this.clone();
        let result = match this.get_direction() {
            Some(Direction::Forward { .. }) => {
                world.try_run_schedule(ForwardSchedule::of(RevUpdate))
            }
            Some(Direction::BackwardLog) => world.try_run_schedule(BackwardSchedule::of(RevUpdate)),
            None => Ok(()),
        };
        if result.is_err() {
            warn_once!("RevMeta cannot find reversible schedule RevUpdate, make sure to not call it or RevMeta::update_world recursively.\n{this:?}");
        }
        if let Some(mut this) = world.get_resource_mut::<Self>() {
            this.direction.end_running();
        }
    }
    pub fn update(&mut self) {
        match self.queue.take() {
            Some(queue) => {
                self.direction = queue;
                match self.get_direction() {
                    Some(Direction::Forward { log: false }) => self.update_forward(),
                    Some(Direction::Forward { log: true }) => self.now += 1,
                    Some(Direction::BackwardLog) => self.now -= 1,
                    None => {}
                }
            }
            None => {
                self.direction.start_running();
                match &mut self.direction {
                    InternalDirection::RunningForward => self.update_forward(),
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
    }
    fn update_forward(&mut self) {
        self.now += 1;
        if self.now >= Self::MAX_FRAME {
            panic!("Maximum reversible timestamp reached: {}", Self::MAX_FRAME)
        }
        self.range.end = self.now + 1;
        if let Some(max_len) = self.max_len {
            self.range.start = self
                .range
                .start
                .max(self.range.end.saturating_sub(max_len.get()));
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
        range: Range<usize>,
        direction: InternalDirection,
    ) -> RevMeta {
        let meta = RevMeta {
            max_len,
            now,
            range: range.clone(),
            direction,
            queue: None,
        };
        assert!(range.start <= now, "{meta:?}");
        match direction {
            InternalDirection::RunningForward => assert_eq!(now, range.end + 1, "{meta:?}"),
            InternalDirection::RunningForwardLog {
                updates_until_pause,
            } => {
                assert!(now < range.end, "{meta:?}");
                assert!(now + updates_until_pause.get() <= range.end, "{meta:?}");
            }
            InternalDirection::RunningBackwardLog {
                updates_until_pause,
            } => {
                assert!(now < range.end, "{meta:?}");
                assert!(
                    range.start + updates_until_pause.get() - 1 <= now,
                    "{meta:?}"
                );
            }
            InternalDirection::Pause => assert!(now < range.end, "{meta:?}"),
            _ => unimplemented!(),
        }
        meta
    }

    #[test]
    fn log_forward_defaults_to_pause() {
        let mut meta = arrange(
            None,
            0,
            0..2,
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
            0..2,
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
        assert_eq!(meta.log_range(), 0..2);

        meta.update();
        assert_eq!(meta.now(), 2);
        assert_eq!(meta.log_range(), 1..3);
    }

    #[test]
    fn queue_log_to_out_of_range_fails() {
        let mut meta = arrange(None, 2, 1..4, InternalDirection::Pause);

        assert_eq!(meta.queue_log(0), Err(1..4));
        assert_eq!(meta.queue_log(4), Err(1..4));
    }

    #[test]
    fn non_log_forward_truncates_future() {
        let mut meta = arrange(
            None,
            2,
            0..3,
            InternalDirection::RunningBackwardLog {
                updates_until_pause: THREE,
            },
        );

        meta.update();
        assert_eq!(meta.now(), 1);
        assert_eq!(meta.log_range(), 0..3);

        meta.update();
        assert_eq!(meta.now(), 0);
        assert_eq!(meta.log_range(), 0..3);

        meta.queue_forward();
        meta.update();
        assert_eq!(meta.now(), 1);
        assert_eq!(meta.log_range(), 0..2);
    }
}
