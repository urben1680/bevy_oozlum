use core::num::NonZeroUsize;
use std::ops::Deref;

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        change_detection::Mut,
        component::ComponentId,
        event::Event,
        query::Access,
        system::{IntoSystem, Res, Resource, System, SystemParam},
        world::World,
    },
    log::warn_once,
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::info,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::{
    commands::init_commands_buffer,
    log::{OutOfLog, PackedRevFrame},
    RevFrame, RevUpdate,
};

mod verifying;

pub use verifying::{VerifyError, VerifyingRevMeta};

#[derive(Clone, Debug, PartialEq)]
pub enum RevTryRunScheduleError {
    RevMetaMissingFirstCall,
    RevMetaMissing { existed_previously: bool },
    RevMetaRemovedInSchedule { meta: RevMeta },
    UnexpectedInitialRunning { meta: RevMeta },
    RevUpdateMissing { meta: RevMeta },
}

#[derive(Clone, Debug)]
pub enum GetFromWorldError {
    // todo: deprecate if unneeded
    RevMetaMissingFirstCall,
    RevMetaMissing { existed_previously: bool },
}

impl From<GetFromWorldError> for RevTryRunScheduleError {
    fn from(value: GetFromWorldError) -> Self {
        match value {
            GetFromWorldError::RevMetaMissingFirstCall => {
                RevTryRunScheduleError::RevMetaMissingFirstCall
            }
            GetFromWorldError::RevMetaMissing { existed_previously } => {
                RevTryRunScheduleError::RevMetaMissing { existed_previously }
            }
        }
    }
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

impl RevDirection {
    #[allow(non_upper_case_globals)]
    pub const NotLog: Self = Self::Forward { log: false };
    #[allow(non_upper_case_globals)]
    pub const ForwardLog: Self = Self::Forward { log: true };
    pub fn is_forward(self) -> bool {
        matches!(self, Self::Forward { .. })
    }
    pub fn is_backward(self) -> bool {
        matches!(self, Self::BackwardLog)
    }
}

unsafe impl SystemParam for RevDirection {
    type Item<'world, 'state> = Self;
    type State = ComponentId;
    fn init_state(
        world: &mut World,
        system_meta: &mut bevy::ecs::system::SystemMeta,
    ) -> Self::State {
        <Res<'static, RevMeta> as SystemParam>::init_state(world, system_meta)
    }
    unsafe fn validate_param(
        state: &Self::State,
        system_meta: &bevy::ecs::system::SystemMeta,
        world: bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell,
    ) -> bool {
        if !<Res<'static, RevMeta> as SystemParam>::validate_param(state, system_meta, world) {
            return false;
        }
        world
            .get_resource_by_id(*state)
            .map(|ptr| ptr.deref::<RevMeta>())
            .and_then(RevMeta::get_direction)
            .is_some()
    }
    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        _system_meta: &bevy::ecs::system::SystemMeta,
        world: bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell<'world>,
        _change_tick: bevy::ecs::component::Tick,
    ) -> Self::Item<'world, 'state> {
        world
            .get_resource_by_id(*state)
            .map(|ptr| ptr.deref::<RevMeta>())
            .and_then(RevMeta::get_direction)
            .unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
enum InternalDirection {
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
    fn get_direction(self) -> Option<RevDirection> {
        match self {
            Self::RunningForward => Some(RevDirection::NotLog),
            Self::RunningForwardLog { .. } => Some(RevDirection::ForwardLog),
            Self::RunningBackwardLog { .. } => Some(RevDirection::BackwardLog),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Event)]
pub struct CheckLoggedAt(RevMeta);

impl Deref for CheckLoggedAt {
    type Target = RevMeta;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Resource, Default)]
pub(crate) struct CommandsLogReducings(pub(crate) Vec<CommandsLogReducingBox>);

pub(crate) type CommandsLogReducingBox = Box<dyn Fn(&RevMeta, &mut World) + Send + Sync + 'static>;

pub enum PastBound {
    PastEndInclusive,
    PastEndExclusive,
    PresentInclusive,
    PresentExclusive,
}

pub enum FutureBound {
    PresentInclusive,
    PresentExclusive,
    FutureEndInclusive,
    FutureEndExclusive,
}

/// RevMeta is used to control the processing of reversible systems.
///
/// It keepts track what the current frame is and to which frame one can go forward and backward in time.
#[derive(Debug, Clone, Resource, Reflect, PartialEq)]
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
    /// Reducing this value alone does not cause deallocations, this has to be done manually with each [log struct](crate::log) if desired.
    ///
    /// Changing this value is always possible but only comes into effect when updating the world during [`RevDirection::NotLog`].
    ///
    /// **Note** that there is a hard limit of [`Self::MAX_WORLD_STATES`] this value is clamped to when read internally.
    pub max_world_states: Option<NonZeroUsize>,
    oldest_frame: RevFrame,
    present_frame: RevFrame,
    youngest_frame: RevFrame,
    /// If Some, is either a Running* variant or Pause
    queue: Option<InternalDirection>,
    direction: InternalDirection,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Some(NonZeroUsize::MIN), 0, false)
    }
}

impl RevMeta {
    pub const MAX_WORLD_STATES: usize = PackedRevFrame::MAX_AS_USIZE / 2;
    pub const fn new(max_len: Option<NonZeroUsize>, now: usize, paused: bool) -> Self {
        if now > PackedRevFrame::MAX_AS_USIZE {
            panic!("now must not be larger than RevMeta::MAX_WORLD_STATES * 2")
        }
        let now = RevFrame::new(now);
        Self {
            max_world_states: max_len,
            present_frame: now,
            oldest_frame: now,
            youngest_frame: now,
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
    pub fn get_direction_after_run(&self) -> Option<RevDirection> {
        let mut direction = self.direction;
        direction.end_running();
        direction.get_direction()
    }
    pub fn present_world_state(&self) -> RevFrame {
        self.present_frame
    }
    pub fn past_world_states(&self) -> usize {
        range_len(self.oldest_frame, self.present_frame)
    }
    pub fn future_world_states(&self) -> usize {
        range_len(self.present_frame, self.youngest_frame)
    }
    pub fn world_states(&self) -> usize {
        let len = range_len(self.oldest_frame, self.youngest_frame);
        len + 1 // both ends are inclusive
    }
    pub fn contains(&self, value: RevFrame) -> bool {
        self.contains_in(
            value,
            PastBound::PastEndInclusive,
            FutureBound::FutureEndInclusive,
        )
    }
    pub(crate) fn contains_in_state_logged(&self, value: RevFrame) -> bool {
        // include present frame in case the log gets pushed into multiple times per frame
        self.contains_in(
            value,
            PastBound::PastEndInclusive,
            FutureBound::PresentInclusive,
        )
    }
    pub(crate) fn contains_in_transition_logged(&self, value: RevFrame) -> bool {
        // include present frame in case the log gets pushed into multiple times per frame
        // exclude oldest frame because transition logs use entries to move to the state before the logged frame
        self.contains_in(
            value,
            PastBound::PastEndExclusive,
            FutureBound::PresentInclusive,
        )
    }
    /// Note that if ...
    ///
    /// - `PastBound::PastEndExclusive` is used and there is no past and/or
    /// - `FutureBound::FutureEndExclusive` is used and there is no future
    ///
    /// ... the evaluation changes as if `PresentInclusive` was used for the bound(s) instead.
    pub fn contains_in(&self, value: RevFrame, past: PastBound, future: FutureBound) -> bool {
        let start_inclusive = match past {
            PastBound::PastEndInclusive => self.oldest_frame,
            PastBound::PastEndExclusive if self.oldest_frame == self.present_frame => {
                self.present_frame
            }
            PastBound::PastEndExclusive => self.oldest_frame.wrapping_add(1),
            PastBound::PresentInclusive => self.present_frame,
            PastBound::PresentExclusive => self.present_frame.wrapping_add(1),
        };
        let end_inclusive = match future {
            FutureBound::PresentInclusive => self.present_frame,
            FutureBound::PresentExclusive => self.present_frame.wrapping_sub(1),
            FutureBound::FutureEndInclusive => self.youngest_frame,
            FutureBound::FutureEndExclusive if self.present_frame == self.youngest_frame => {
                self.present_frame
            }
            FutureBound::FutureEndExclusive => self.youngest_frame.wrapping_sub(1),
        };
        let states = range_len(start_inclusive, end_inclusive);
        if states > Self::MAX_WORLD_STATES {
            return false; // start_inclusive > end_inclusive
        }
        let states_to_value_inclusive = range_len(start_inclusive, value);
        states_to_value_inclusive <= states
    }
    pub(crate) fn frames_since(&self, frame: RevFrame) -> usize {
        range_len(frame, self.present_frame)
    }
    pub fn clear(&mut self) {
        self.oldest_frame = self.present_frame;
        self.youngest_frame = self.present_frame;
    }
    /// Queue to go forward.
    ///
    /// Will cause logged future frames to be forgotten.
    pub fn queue_forward(&mut self) {
        self.queue = Some(InternalDirection::RunningForward);
    }
    pub fn queue_log(&mut self, to: RevFrame) -> Result<usize, OutOfLog> {
        let to_past = range_len(to, self.present_frame);
        let to_future = range_len(self.present_frame, to);
        if to_past > self.past_world_states() && to_future > self.future_world_states() {
            return Err(OutOfLog);
        }
        let from_present = to_past.min(to_future);
        self.queue = NonZeroUsize::new(from_present).map(|updates_until_pause| {
            if to_past == from_present {
                InternalDirection::RunningBackwardLog {
                    updates_until_pause,
                }
            } else {
                InternalDirection::RunningForwardLog {
                    updates_until_pause,
                }
            }
        });
        Ok(from_present)
    }
    pub fn queue_pause(&mut self) {
        self.queue = Some(InternalDirection::Pause);
    }
    pub fn try_update_world(world: &mut World) -> Result<(), RevTryRunScheduleError> {
        #[derive(Resource, Clone, Copy)]
        struct Existed(bool);

        if world.contains_resource::<Self>() {
            world.insert_resource(Existed(true));
        } else {
            let err = match world.get_resource::<Existed>().cloned() {
                None => RevTryRunScheduleError::RevMetaMissingFirstCall,
                Some(Existed(existed_previously)) => {
                    RevTryRunScheduleError::RevMetaMissing { existed_previously }
                }
            };
            world.insert_resource(Existed(false));
            return Err(err);
        }

        world.resource_scope(|world: &mut World, mut meta: Mut<Self>| {
            if meta.get_direction().is_some() {
                return Err(RevTryRunScheduleError::UnexpectedInitialRunning {
                    meta: meta.clone(),
                });
            }
            let previous = meta.clone();
            let result = meta.update(|meta, reduce_logged_at| {
                world
                    .try_schedule_scope(RevUpdate, |world, schedule| {
                        // While this only needs to be once, this is the only resource that must be initialized before the first
                        // RevUpdate run because the first access may be by a hook that only has DeferredWorld and cannot add it itself.
                        // With this being done here bevy-ecs only users cannot forget to init it.
                        init_commands_buffer(world);

                        world.insert_resource(meta.clone());

                        if let Some(reduce_logged_at) = reduce_logged_at {
                            world.trigger(reduce_logged_at);
                            if let Some(reducings) = world.remove_resource::<CommandsLogReducings>()
                            {
                                for reducing in &reducings.0 {
                                    reducing(meta, world);
                                }
                                world.insert_resource(reducings);
                            }
                        }

                        schedule.run(world);
                    })
                    .map_err(|_| RevTryRunScheduleError::RevUpdateMissing { meta: meta.clone() })
            });

            match result.transpose() {
                Ok(_) => {
                    let Some(updated) = world.remove_resource::<Self>() else {
                        return Err(RevTryRunScheduleError::RevMetaRemovedInSchedule {
                            meta: meta.clone(),
                        });
                    };
                    meta.max_world_states = updated.max_world_states;
                    meta.queue = updated.queue;
                    Ok(())
                }
                Err(err) => {
                    world.remove_resource::<Self>();
                    *meta = previous;
                    Err(err)
                }
            }
        })
    }
    pub fn update_world(world: &mut World) {
        match Self::try_update_world(world) {
            Err(RevTryRunScheduleError::RevMetaMissingFirstCall) => info!(
                "RevMeta does not exist yet, reversible schedule RevUpdate will not be called until it is inserted"
            ),
            Err(RevTryRunScheduleError::RevMetaMissing { existed_previously: true, .. }) => info!(
                "RevMeta was removed, reversible schedule RevUpdate will not be called until it is inserted again"
            ),
            Err(RevTryRunScheduleError::RevUpdateMissing { .. }) => warn_once!(
                "RevMeta cannot find reversible schedule RevUpdate, make sure to not call RevMeta::update_world recursively"
            ),
            _ => {}
        }
    }
    /// Updates `RevMeta`. The closure is called if the updated direction is not paused.
    ///
    /// The given `&mut RevMeta` in the closure has the following characteristics:
    ///
    /// - the mutable reference allows to queue the next direction
    /// - [`get_direction`](Self::get_direction) always returns `Some`, therefore [`direction`](Self::direction) can be used instead
    /// - the value behind it should not be swaped with another instance of `RevMeta`
    ///
    /// If the second closure argument is `Some`, logs that track their entries via [`LoggedAt`](crate::log::LoggedAt)
    /// and are not calling their `pop_past_by_logged_at` every time in this closure, trigger their
    /// `truncate_future_drain_past_by_logged_at` method instead then.
    ///
    /// If this method is not manually called and instead ([`try_`](Self::try_update_world))[`update_world`](Self::update_world)
    /// is used as a system to call the [`RevUpdate`] schedule, the above mechanism is triggered via an observer.
    /// See [`CheckLoggedAt`]
    ///
    /// # Panics
    ///
    /// If this is called recursively in the closure and the closure is called because the updated direction is not paused,
    /// this will panic. The same can happen if `RevMeta` is in an invalid state, cloned from inside the closure for example.
    pub fn update<Out>(
        &mut self,
        c: impl FnOnce(&mut Self, Option<CheckLoggedAt>) -> Out,
    ) -> Option<Out> {
        if self.get_direction().is_some() {
            panic!("unexpected initial direction, expected pause or ran variant, do not call this method recursively\n{self:#?}");
        }
        let reduce_logged_at = self.update_internal();
        self.get_direction().map(|_| {
            let out = c(self, reduce_logged_at);
            self.direction.end_running();
            out
        })
    }
    fn update_internal(&mut self) -> Option<CheckLoggedAt> {
        match self.queue.take() {
            Some(queue) => {
                self.direction = queue;
                self.present_frame = match self.get_direction() {
                    Some(RevDirection::NotLog) => return self.update_forward(),
                    Some(RevDirection::ForwardLog) => self.present_frame.wrapping_add(1),
                    Some(RevDirection::BackwardLog) => self.present_frame.wrapping_sub(1),
                    None => self.present_frame,
                };
            }
            None => {
                self.direction.start_running();
                let updated_at_log = match &mut self.direction {
                    InternalDirection::RunningForward => return self.update_forward(),
                    InternalDirection::RunningForwardLog {
                        updates_until_pause,
                    } => reduction_successful(updates_until_pause)
                        .then(|| self.present_frame.wrapping_add(1)),
                    InternalDirection::RunningBackwardLog {
                        updates_until_pause,
                    } => reduction_successful(updates_until_pause)
                        .then(|| self.present_frame.wrapping_sub(1)),
                    _ /* Pause */ => None,
                };
                match updated_at_log {
                    Some(updated) => self.present_frame = updated,
                    None => self.direction = InternalDirection::Pause,
                }
            }
        }
        None
    }
    fn update_forward(&mut self) -> Option<CheckLoggedAt> {
        self.present_frame = self.present_frame.wrapping_add(1);
        let max_world_states = self
            .max_world_states
            .map(NonZeroUsize::get)
            .unwrap_or(Self::MAX_WORLD_STATES)
            .min(Self::MAX_WORLD_STATES);
        // past states equal to max states is too many as the present state has to be added to the comparision
        if self.past_world_states() >= max_world_states {
            self.oldest_frame = self.present_frame.wrapping_sub(max_world_states - 1);
        }
        self.youngest_frame = self.present_frame;
        matches!(
            self.present_frame.0,
            Self::MAX_WORLD_STATES | PackedRevFrame::MAX_AS_USIZE
        )
        .then_some(CheckLoggedAt(self.clone()))
    }
    #[cfg(test)]
    pub(crate) fn set_oldest_frame(&mut self, oldest_frame: usize) {
        self.oldest_frame = RevFrame::new(oldest_frame);
        let past_world_states = self.past_world_states();
        if self
            .max_world_states
            .is_some_and(|max_world_states| max_world_states.get() < past_world_states)
        {
            self.max_world_states = NonZeroUsize::new(past_world_states);
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

/// Returns len of wrapping range `start..end`
fn range_len(start: RevFrame, end: RevFrame) -> usize {
    if PackedRevFrame::MAX_AS_USIZE != usize::MAX && start.0 > end.0 {
        // 0 ## end .. start ## PackedRevFrame::MAX_AS_USIZE .. usize::MAX
        PackedRevFrame::MAX_AS_USIZE - start.0 + end.0
    } else {
        // 0 .. start ## end .. PackedRevFrame::MAX_AS_USIZE .. usize::MAX
        end.0.wrapping_sub(start.0)
    }
}

#[cfg(test)]
mod test {
    use std::ops::RangeInclusive;

    use bevy::prelude::{ResMut, Schedule, Schedules, Trigger};

    use crate::log::TransitionLog;

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
        let present_world_state = RevFrame::new(now);
        let oldest_world_state = RevFrame::new(*range.start());
        let youngest_world_state = RevFrame::new(*range.end());
        let meta = RevMeta {
            max_world_states: max_len,
            present_frame: present_world_state,
            oldest_frame: oldest_world_state,
            youngest_frame: youngest_world_state,
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
                assert!(
                    range.start() + updates_until_pause.get() - 1 <= now,
                    "{meta:?}"
                );
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

        meta.update_internal();
        assert_eq!(
            meta.direction,
            InternalDirection::RunningForwardLog {
                updates_until_pause: ONE
            }
        );
        assert_eq!(meta.present_frame.0, 1);

        meta.update_internal();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.present_frame.0, 1);
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

        meta.update_internal();
        assert_eq!(
            meta.direction,
            InternalDirection::RunningBackwardLog {
                updates_until_pause: ONE
            }
        );
        assert_eq!(meta.present_frame.0, 0);

        meta.update_internal();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.present_frame.0, 0);
    }

    #[test]
    fn start_grows_according_to_max_len() {
        let mut meta = RevMeta::new(Some(TWO), 0, false);

        meta.update_internal();
        assert_eq!(meta.present_frame.0, 1);
        assert_eq!(meta.world_states(), 2);

        meta.update_internal();
        assert_eq!(meta.present_frame.0, 2);
        assert_eq!(meta.world_states(), 2);
    }

    #[test]
    fn queue_log_to_out_of_range_fails() {
        let mut meta = arrange(None, 2, 1..=3, InternalDirection::Pause);

        assert_eq!(meta.queue_log(RevFrame::new(0)), Err(OutOfLog));
        assert_eq!(meta.queue_log(RevFrame::new(4)), Err(OutOfLog));
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

        meta.update_internal();
        meta.update_internal();
        assert_eq!(meta.world_states(), 3);
        meta.queue_forward();
        meta.update_internal();
        assert_eq!(meta.world_states(), 2);
    }

    #[test]
    fn check_logged_at() {
        #[derive(Resource)]
        struct CheckLoggedAtRes(TransitionLog<RevFrame>);

        let meta = RevMeta::new(None, RevMeta::MAX_WORLD_STATES - 1, false);
        let mut res = CheckLoggedAtRes(TransitionLog::new());
        res.0.push_present(RevFrame::new(0));
        assert_eq!(res.0.transitions_len(), 1);

        let mut schedules = Schedules::new();
        schedules.insert(Schedule::new(RevUpdate));

        let mut world = World::new();
        world.insert_resource(meta);
        world.insert_resource(res);
        world.insert_resource(schedules);
        world.add_observer(
            |trigger: Trigger<CheckLoggedAt>, mut res: ResMut<CheckLoggedAtRes>| {
                res.0.pop_past_by_logged_at(trigger.event());
            },
        );
        world.flush();

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(()));
        let res = world.remove_resource::<CheckLoggedAtRes>().unwrap();
        assert_eq!(res.0.transitions_len(), 0);
    }
}
