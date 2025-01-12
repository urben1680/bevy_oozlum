use core::num::NonZeroU32;

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        change_detection::Mut,
        component::{ComponentId, Tick},
        event::Event,
        query::Access,
        system::{IntoSystem, ReadOnlySystemParam, Res, Resource, System, SystemMeta, SystemParam},
        world::{unsafe_world_cell::UnsafeWorldCell, World},
    },
    log::warn_once,
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::info,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::{
    frame::{RevFrame, REV_FRAME_AS_U32_MAX},
    log::OutOfLog,
    schedule::RevUpdate,
    undo_redo::UndoRedoBuffer,
};

#[derive(Clone, Debug, PartialEq)]
pub enum RevTryRunScheduleError {
    RevMetaMissingFirstCall,
    RevMetaMissing { existed_previously: bool },
    RevMetaRemovedInSchedule { meta: RevMeta },
    UnexpectedInitialRunning { meta: RevMeta },
    RevUpdateMissing { meta: RevMeta },
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
    pub const NOT_LOG: Self = Self::Forward { log: false };
    pub const FORWARD_LOG: Self = Self::Forward { log: true };
    pub fn is_forward(self) -> bool {
        self != Self::BackwardLog
    }
    pub fn is_log(self) -> bool {
        self != Self::Forward { log: false }
    }
}

// SAFETY: todo
unsafe impl SystemParam for RevDirection {
    type Item<'world, 'state> = Self;
    type State = ComponentId;
    fn init_state(world: &mut World, system_meta: &mut SystemMeta) -> Self::State {
        <Res<RevMeta> as SystemParam>::init_state(world, system_meta)
    }
    // todo: update implementation and doc for bevy 0.16 as the behavior of Res changes then again
    unsafe fn validate_param(
        &component_id: &Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell,
    ) -> bool {
        // SAFETY: Read-only access is registered in init_state for this id and ptr read access is finished before return.
        let Some(ptr) = world.get_resource_by_id(component_id) else {
            system_meta.try_warn_param::<Res<RevMeta>>();
            return false;
        };
        if ptr.deref::<RevMeta>().get_direction().is_none() {
            system_meta.try_warn_param::<Self>();
            return false;
        }
        true
    }
    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        _system_meta: &SystemMeta,
        world: UnsafeWorldCell<'world>,
        _change_tick: Tick,
    ) -> Self::Item<'world, 'state> {
        world
            .get_resource_by_id(*state)
            .map(|ptr| ptr.deref::<RevMeta>())
            .unwrap()
            .direction()
    }
}

// SAFETY: only reads RevMeta resource
unsafe impl ReadOnlySystemParam for RevDirection {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
enum InternalDirection {
    RunningForward,
    RunningForwardLog { updates_until_pause: NonZeroU32 },
    RunningBackwardLog { updates_until_pause: NonZeroU32 },
    RanForward,
    RanForwardLog { updates_until_pause: NonZeroU32 },
    RanBackwardLog { updates_until_pause: NonZeroU32 },
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
            Self::RunningForward => Some(RevDirection::NOT_LOG),
            Self::RunningForwardLog { .. } => Some(RevDirection::FORWARD_LOG),
            Self::RunningBackwardLog { .. } => Some(RevDirection::BackwardLog),
            _ => None,
        }
    }
    fn ran_direction(self) -> Option<RevDirection> {
        match self {
            Self::RanForward => Some(RevDirection::NOT_LOG),
            Self::RanForwardLog { .. } => Some(RevDirection::FORWARD_LOG),
            Self::RanBackwardLog { .. } => Some(RevDirection::BackwardLog),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Event)]
pub struct DrainPastByLoggedAt(RevMeta);

impl DrainPastByLoggedAt {
    pub fn meta(&self) -> &RevMeta {
        &self.0
    }
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
    pub max_world_states: Option<NonZeroU32>,
    past_end: RevFrame,
    present: RevFrame,
    future_end: RevFrame,
    /// If Some, is either a Running* variant or Pause
    queue: Option<InternalDirection>,
    direction: InternalDirection,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Some(NonZeroU32::MIN), None, false)
    }
}

impl RevMeta {
    pub const MAX_WORLD_STATES: u32 = REV_FRAME_AS_U32_MAX / 2;
    pub const fn new(
        max_world_states: Option<NonZeroU32>,
        now: Option<RevFrame>,
        paused: bool,
    ) -> Self {
        let now = match now {
            Some(now) => now,
            None => RevFrame(0),
        };
        Self {
            max_world_states,
            present: now,
            past_end: now,
            future_end: now,
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
    pub fn ran_direction(&self) -> Option<RevDirection> {
        self.direction.ran_direction()
    }
    pub fn paused(&self) -> bool {
        self.direction == InternalDirection::Pause
    }
    pub fn future_end_world_state(&self) -> RevFrame {
        self.future_end
    }
    pub fn present_world_state(&self) -> RevFrame {
        self.present
    }
    pub fn past_end_world_state(&self) -> RevFrame {
        self.past_end
    }
    pub fn past_world_states(&self) -> u32 {
        self.present - self.past_end
    }
    pub fn future_world_states(&self) -> u32 {
        self.future_end - self.present
    }
    pub fn world_states(&self) -> u32 {
        self.future_end - self.past_end + 1 // both ends are inclusive
    }
    pub fn contains(&self, frame: RevFrame) -> bool {
        self.contains_in(frame, true, true)
    }
    // todo: no longer needed to have that many options, deprecate
    pub fn contains_in(
        &self,
        frame: RevFrame,
        past_end_inclusive: bool,
        future_end_inclusive: bool,
    ) -> bool {
        if frame == self.past_end {
            return past_end_inclusive;
        }
        if frame == self.future_end {
            return future_end_inclusive;
        }
        (self.future_end - frame) < (self.future_end - self.past_end)
    }
    // todo: no longer needed to have that many options, simplify
    pub fn contains_in_past(
        &self,
        frame: RevFrame,
        past_end_inclusive: bool,
        present_inclusive: bool,
    ) -> bool {
        if frame == self.past_end {
            return past_end_inclusive;
        }
        if frame == self.present {
            return present_inclusive;
        }
        (self.present - frame) < (self.present - self.past_end)
    }
    // todo: no longer needed to have that many options, simplify
    pub fn contains_in_future(
        &self,
        frame: RevFrame,
        present_inclusive: bool,
        future_end_inclusive: bool,
    ) -> bool {
        if frame == self.present {
            return present_inclusive;
        }
        if frame == self.future_end {
            return future_end_inclusive;
        }
        (self.future_end - frame) < (self.future_end - self.present)
    }
    pub fn frame_cmp(&self, lhs: RevFrame, rhs: RevFrame) -> std::cmp::Ordering {
        (self.past_end - lhs).cmp(&(self.past_end - rhs))
    }
    pub fn clear(&mut self) {
        self.past_end = self.present;
        self.future_end = self.present;
    }
    /// Queue to go forward.
    ///
    /// Will cause logged future frames to be forgotten.
    pub fn queue_forward(&mut self) {
        self.queue = Some(InternalDirection::RunningForward);
    }
    pub fn queue_log(&mut self, to: RevFrame) -> Result<u32, OutOfLog> {
        let to_past = self.present - to;
        let to_future = to - self.present;

        if to_past <= self.past_world_states() {
            self.queue = Some(match NonZeroU32::new(to_past) {
                Some(updates_until_pause) => InternalDirection::RunningBackwardLog {
                    updates_until_pause,
                },
                None => InternalDirection::Pause,
            });
            Ok(to_past)
        } else if to_future <= self.future_world_states() {
            self.queue = Some(match NonZeroU32::new(to_future) {
                Some(updates_until_pause) => InternalDirection::RunningForwardLog {
                    updates_until_pause,
                },
                None => InternalDirection::Pause,
            });
            Ok(to_future)
        } else {
            Err(OutOfLog)
        }
    }
    pub fn queue_pause(&mut self) {
        self.queue = Some(InternalDirection::Pause);
    }
    pub fn try_run_rev_update(
        world: &mut World,
    ) -> Result<Option<UndoRedoBuffer>, RevTryRunScheduleError> {
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
            let buffer = world
                .get_resource_mut::<UndoRedoBuffer>()
                .is_some_and(|buffer| !buffer.is_empty())
                .then(|| world.remove_resource::<UndoRedoBuffer>().unwrap());
            world.init_resource::<UndoRedoBuffer>();

            if meta.get_direction().is_some() {
                return Err(RevTryRunScheduleError::UnexpectedInitialRunning {
                    meta: meta.clone(),
                });
            }
            let previous = meta.clone();
            let result = meta.update(|meta, drain_past_by_logged_at| {
                world
                    .try_schedule_scope(RevUpdate, |world, schedule| {
                        world.insert_resource(meta.clone());

                        if let Some(drain_past_by_logged_at) = drain_past_by_logged_at {
                            world.trigger(drain_past_by_logged_at);
                            world.flush();
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
                    Ok(buffer)
                }
                Err(err) => {
                    world.remove_resource::<Self>();
                    *meta = previous;
                    Err(err)
                }
            }
        })
    }
    pub fn run_rev_update(world: &mut World) {
        match Self::try_run_rev_update(world) {
            Err(RevTryRunScheduleError::RevMetaMissingFirstCall) => info!(
                "RevMeta does not exist yet, reversible schedule RevUpdate will not be called until it is inserted"
            ),
            Err(RevTryRunScheduleError::RevMetaMissing { existed_previously: true, .. }) => info!(
                "RevMeta was removed, reversible schedule RevUpdate will not be called until it is inserted again"
            ),
            Err(RevTryRunScheduleError::RevUpdateMissing { .. }) => warn_once!(
                "RevMeta cannot find reversible schedule RevUpdate, make sure to not call RevMeta::update_world recursively"
            ),
            Ok(Some(_)) => warn_once!(
                "`UndoRedoBuffer` was discovered non-empty at the start of the reversible schedule run"
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
    /// See [`DrainPastByLoggedAt`]
    ///
    /// # Panics
    ///
    /// If this is called recursively in the closure and the closure is called because the updated direction is not paused,
    /// this will panic. The same can happen if `RevMeta` is in an invalid state, cloned from inside the closure for example.
    pub fn update<Out>(
        &mut self,
        c: impl FnOnce(&mut Self, Option<DrainPastByLoggedAt>) -> Out,
    ) -> Option<Out> {
        if self.get_direction().is_some() {
            panic!("unexpected initial direction, expected pause or ran variant, do not call this method recursively\n{self:#?}");
        }
        let drain_past_by_logged_at = self.update_internal();
        self.get_direction().map(|_| {
            let out = c(self, drain_past_by_logged_at);
            self.direction.end_running();
            out
        })
    }
    fn update_internal(&mut self) -> Option<DrainPastByLoggedAt> {
        /// Reduces `updates_until_pause` by one and returns `true` wether that was successful without reaching zero.
        fn reduction_successful(updates_until_pause: &mut NonZeroU32) -> bool {
            NonZeroU32::new(updates_until_pause.get() - 1)
                .map(|reduced| *updates_until_pause = reduced)
                .is_some()
        }

        match self.queue.take() {
            Some(queue) => {
                self.direction = queue;
                self.present = match self.get_direction() {
                    Some(RevDirection::NOT_LOG) => return self.update_forward(),
                    Some(RevDirection::FORWARD_LOG) => self.present.wrapping_add(1),
                    Some(RevDirection::BackwardLog) => self.present.wrapping_sub(1),
                    None => self.present,
                };
            }
            None => {
                self.direction.start_running();
                let updated_at_log = match &mut self.direction {
                    InternalDirection::RunningForward => return self.update_forward(),
                    InternalDirection::RunningForwardLog {
                        updates_until_pause,
                    } => reduction_successful(updates_until_pause)
                        .then(|| self.present.wrapping_add(1)),
                    InternalDirection::RunningBackwardLog {
                        updates_until_pause,
                    } => reduction_successful(updates_until_pause)
                        .then(|| self.present.wrapping_sub(1)),
                    _ /* Pause */ => None,
                };
                match updated_at_log {
                    Some(updated) => self.present = updated,
                    None => self.direction = InternalDirection::Pause,
                }
            }
        }
        None
    }
    fn update_forward(&mut self) -> Option<DrainPastByLoggedAt> {
        self.present = self.present.wrapping_add(1);
        let max_world_states = self
            .max_world_states
            .map(NonZeroU32::get)
            .unwrap_or(Self::MAX_WORLD_STATES)
            .min(Self::MAX_WORLD_STATES);
        self.future_end = self.present;
        // past states equal to max states is too many as the present state has to be added to the comparision
        if self.past_world_states() >= max_world_states {
            let first_half = self.past_end.first_half();
            self.past_end = self.present.wrapping_sub(max_world_states - 1);
            if self.past_end.first_half() != first_half {
                return Some(DrainPastByLoggedAt(self.clone()));
            }
        }
        None
    }
    #[cfg(test)]
    pub(crate) fn set_oldest_frame(&mut self, oldest_frame: u32) {
        self.past_end = RevFrame::checked_new(oldest_frame);
        let past_world_states = self.past_world_states();
        if self
            .max_world_states
            .is_some_and(|max_world_states| max_world_states.get() < past_world_states)
        {
            self.max_world_states = NonZeroU32::new(past_world_states);
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

/*
Testing different approach of periodic cleanup:
- lazy, done by user
- downsides:
-- is called every non-log update
- upsides:
-- easier to do than observers
-- rare logs likely do not run often enough for extra call to matter

max log len <= 4
half:  first  | second
frame: 0 1 2 3|4 5 6 7
meta:  # # # #|# # # #
       # # # #|# 5 6 # cleanup id 3, log start 5, present 6
log1:  0 s 2 #|# # # #
       # # # #|# # 6 # cleanup id 0, should be updated and then cleaned
log2:  # # # #|# # # #
       # 1 # 3|4 # 6 # cleanup id 2, can be updated any order to clean which reduces part of log
log3:  # # # #|# # # #
       # # # #|4 # 6 # cleanup id 3, can be updated any order to clean which reduces part of log

splitting in halves may be not needed, cleanup id could be the # of times the log start overflowed

fixed len logs remain as they are

logged at changes:
- if own log start has same id
-- check from start how much to drain, be aware overflow may happen along the way
- if own log start has 1 less id
-- find overflow in log, check from here how much to drain
- if own log start has >1 less id
-- clean entire past

where to store id in log?
TLog:
TsLog:
RareTLog:
RareTsLog:

all a RareLog next to it?

It might be more economic to just behave like fixed len logs? 
robably not with extreme examples of huge T or U

*/

#[cfg(test)]
mod test {
    use std::ops::RangeInclusive;

    use bevy::ecs::{
        observer::Trigger,
        schedule::{Schedule, Schedules},
        system::ResMut,
    };

    use crate::{log::TransitionLog, undo_redo::UndoRedoBuffer};

    use super::*;

    const ONE: NonZeroU32 = NonZeroU32::MIN;
    const TWO: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(2) };
    const THREE: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(3) };

    /// Constructs [`RevMeta`] and asserts the values are valid
    fn arrange(
        max_len: Option<NonZeroU32>,
        now: u32,
        range: RangeInclusive<u32>,
        direction: InternalDirection,
    ) -> RevMeta {
        let present = RevFrame::checked_new(now);
        let past_end = RevFrame::checked_new(*range.start());
        let future_end = RevFrame::checked_new(*range.end());
        let meta = RevMeta {
            max_world_states: max_len,
            present,
            past_end,
            future_end,
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
        assert_eq!(meta.present.0, 1);

        meta.update_internal();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.present.0, 1);
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
        assert_eq!(meta.present.0, 0);

        meta.update_internal();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.present.0, 0);
    }

    #[test]
    fn start_grows_according_to_max_len() {
        let mut meta = RevMeta::new(Some(TWO), None, false);

        meta.update_internal();
        assert_eq!(meta.present.0, 1);
        assert_eq!(meta.world_states(), 2);

        meta.update_internal();
        assert_eq!(meta.present.0, 2);
        assert_eq!(meta.world_states(), 2);
    }

    #[test]
    fn queue_log_to_out_of_range_fails() {
        let mut meta = arrange(None, 2, 1..=3, InternalDirection::Pause);

        assert_eq!(meta.queue_log(RevFrame::checked_new(0)), Err(OutOfLog));
        assert_eq!(meta.queue_log(RevFrame::checked_new(4)), Err(OutOfLog));
    }

    #[test]
    fn queue_log_to_in_range_succeeds() {
        let mut meta = arrange(None, 2, 1..=3, InternalDirection::Pause);

        assert_eq!(meta.queue_log(RevFrame::checked_new(1)), Ok(1));
        assert_eq!(meta.queue_log(RevFrame::checked_new(3)), Ok(1));
    }

    #[test]
    fn queue_log_to_present_pauses() {
        let mut meta = arrange(
            None,
            2,
            1..=3,
            InternalDirection::RunningForwardLog {
                updates_until_pause: NonZeroU32::new(2).unwrap(),
            },
        );
        assert_eq!(meta.queue_log(RevFrame::checked_new(2)), Ok(0));
        meta.update_internal();
        assert!(meta.paused());
    }

    #[test]
    fn contains_returns_expected() {
        let meta = arrange(None, 3, 1..=5, InternalDirection::Pause);

        let frame: [RevFrame; 7] = std::array::from_fn(|n| RevFrame::checked_new(n as u32));

        let contains = || {
            assert_eq!(meta.contains(frame[0]), false, "{meta:#?}");
            assert_eq!(meta.contains(frame[1]), true, "{meta:#?}");
            assert_eq!(meta.contains(frame[2]), true, "{meta:#?}");
            assert_eq!(meta.contains(frame[3]), true, "{meta:#?}");
            assert_eq!(meta.contains(frame[4]), true, "{meta:#?}");
            assert_eq!(meta.contains(frame[5]), true, "{meta:#?}");
            assert_eq!(meta.contains(frame[6]), false, "{meta:#?}");
        };

        let contains_in = |past_end_inclusive, future_end_inclusive| {
            let test = |n: usize, expected: bool| {
                assert_eq!(
                meta.contains_in(frame[n], past_end_inclusive, future_end_inclusive),
                expected, "past_end_inclusive:{past_end_inclusive}, future_end_inclusive:{future_end_inclusive}\n{meta:#?}"
            )
            };
            test(0, false);
            test(1, past_end_inclusive);
            test(2, true);
            test(3, true);
            test(4, true);
            test(5, future_end_inclusive);
            test(6, false);
        };

        let contains_in_past = |past_end_inclusive, present_inclusive| {
            let test = |n: usize, expected: bool| {
                assert_eq!(
                meta.contains_in_past(frame[n], past_end_inclusive, present_inclusive),
                expected, "past_end_inclusive:{past_end_inclusive}, present_inclusive:{present_inclusive}\n{meta:#?}"
            )
            };
            test(0, false);
            test(1, past_end_inclusive);
            test(2, true);
            test(3, present_inclusive);
            test(4, false);
            test(5, false);
            test(6, false);
        };

        let contains_in_future = |present_inclusive, future_end_inclusive| {
            let test = |n: usize, expected: bool| {
                assert_eq!(
                meta.contains_in_future(frame[n], present_inclusive, future_end_inclusive),
                expected, "present_inclusive:{present_inclusive}, future_end_inclusive:{future_end_inclusive}\n{meta:#?}"
            )
            };
            test(0, false);
            test(1, false);
            test(2, false);
            test(3, present_inclusive);
            test(4, true);
            test(5, future_end_inclusive);
            test(6, false);
        };

        contains();
        contains_in(false, false);
        contains_in(false, true);
        contains_in(true, false);
        contains_in(true, true);
        contains_in_past(false, false);
        contains_in_past(false, true);
        contains_in_past(true, false);
        contains_in_past(true, true);
        contains_in_future(false, false);
        contains_in_future(false, true);
        contains_in_future(true, false);
        contains_in_future(true, true);
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
        assert_eq!(meta.world_states(), 3, "{meta:#?}");
        meta.queue_forward();
        meta.update_internal();
        assert_eq!(meta.world_states(), 2, "{meta:#?}");
    }

    #[test]
    fn drain_past_by_logged_at() {
        #[derive(Resource)]
        struct DrainPastByLoggedAtRes(TransitionLog<RevFrame>);

        let now = RevFrame::checked_new(RevMeta::MAX_WORLD_STATES - 1);
        let mut meta = RevMeta::new(NonZeroU32::new(1), Some(now), false);
        meta.update(|_, _| ()); // bring oldest_state to edge of first half
        let mut res = DrainPastByLoggedAtRes(TransitionLog::new());
        res.0.push_present(RevFrame::checked_new(0));
        assert_eq!(res.0.transitions_len(), 1);

        let mut schedules = Schedules::new();
        schedules.insert(Schedule::new(RevUpdate));

        let mut world = World::new();
        world.init_resource::<UndoRedoBuffer>();
        world.insert_resource(meta);
        world.insert_resource(res);
        world.insert_resource(schedules);
        world.add_observer(
            |trigger: Trigger<DrainPastByLoggedAt>, mut res: ResMut<DrainPastByLoggedAtRes>| {
                res.0.pop_past_by_logged_at(trigger.event().meta());
            },
        );
        world.flush();

        assert!(RevMeta::try_run_rev_update(&mut world).is_ok());
        let res = world.remove_resource::<DrainPastByLoggedAtRes>().unwrap();
        assert_eq!(res.0.transitions_len(), 0);
    }
}
