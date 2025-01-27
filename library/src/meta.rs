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
    frame::{PackedRevFrame, RevFrame, RevFrameNew, REV_FRAME_AS_U32_MAX},
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

// todo: deprecate
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
    past_end: RevFrameNew,
    present: RevFrameNew,
    future_end: RevFrameNew,
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
        now: Option<RevFrameNew>,
        paused: bool,
    ) -> Self {
        let now = match now {
            Some(now) => now,
            None => RevFrameNew::from_raw(0),
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
    pub fn future_end_world_state(&self) -> RevFrameNew {
        self.future_end
    }
    pub fn present_world_state(&self) -> RevFrameNew {
        self.present
    }
    pub fn past_end_world_state(&self) -> RevFrameNew {
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
    pub fn contains(&self, frame: RevFrameNew) -> bool {
        (self.future_end - frame) <= (self.future_end - self.past_end)
    }
    // todo: no longer needed to have that many options, simplify
    pub fn past_contains(&self, frame: RevFrameNew) -> bool {
        (self.present - frame).wrapping_sub(1) < (self.present - self.past_end)
    }
    // todo: no longer needed to have that many options, simplify
    pub fn future_contains(&self, frame: RevFrameNew) -> bool {
        (self.future_end - frame) < (self.future_end - self.present)
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
    pub fn queue_log(&mut self, to: RevFrameNew) -> Result<u32, OutOfLog> {
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
            let result = meta.update(|meta| {
                world
                    .try_schedule_scope(RevUpdate, |world, schedule| {
                        world.insert_resource(meta.clone());
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
        c: impl FnOnce(&mut Self) -> Out,
    ) -> Option<Out> {
        if self.get_direction().is_some() {
            panic!("unexpected initial direction, expected pause or ran variant, do not call this method recursively\n{self:#?}");
        }
        self.update_internal();
        self.get_direction().map(|_| {
            let out = c(self);
            self.direction.end_running();
            out
        })
    }
    fn update_internal(&mut self) {
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
                    Some(RevDirection::FORWARD_LOG) => self.present.increase_frame(),
                    Some(RevDirection::BackwardLog) => self.present.decrease_frame(),
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
                        .then(|| self.present.increase_frame()),
                    InternalDirection::RunningBackwardLog {
                        updates_until_pause,
                    } => reduction_successful(updates_until_pause)
                        .then(|| self.present.decrease_frame()),
                    _ /* Pause */ => None,
                };
                match updated_at_log {
                    Some(updated) => self.present = updated,
                    None => self.direction = InternalDirection::Pause,
                }
            }
        }
    }
    fn update_forward(&mut self) {
        self.present = self.present.increase_frame();
        let max_world_states = self
            .max_world_states
            .map(NonZeroU32::get)
            .unwrap_or(Self::MAX_WORLD_STATES)
            .min(Self::MAX_WORLD_STATES);
        self.future_end = self.present;
        // past states equal to max states is too many as the present state has to be added to the comparision
        if self.past_world_states() >= max_world_states {
            self.past_end = self.past_end.sub_by_frames(max_world_states - 1);
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

deprecate Rare logs with logged_at functionality
logged_at functionality in new log wrapper

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
but then the contains check is harder/impossible?

fixed len logs remain as they are

logged at changes:
- if own log start has same id
-- check from start how much to drain, be aware overflow may happen along the way
- if own log start has 1 less id
-- find overflow in log, check from here how much to drain
- if own log start has >1 less id
-- clean entire past

because logic is moved to push place, the log methods need to be merged, push + drains
complicated drain logic undesired because vec_deque::Drain cannot be stored with &mut VecDeque
so push always happens first, then a drain is returned

for entry in log.future_drain() { ... }
for entry in log.push_drain_past(("abs", now)) { ... }

since there is no VecDeque::truncate_front, no push method without drain is useful
borrow checker wont complain if drain is not assigned to a variable

because push+drain is combined, there can be no overflow in the log after the call,
the overflow id is valid for the whole log including present
                limited by  push
DenseStateLog   len         push_drain_past(max_len, T) -> Option<T>
DenseStatesLog  len         push_drain_past(max_len, impl FnOnce(LogMut) -> U) -> Drain
                            try_push_drain_past(max_len, impl FnOnce(LogMut) -> U) -> Result<Drain, AmountErr>
SparseStateLog  len         push_drain_past(max_len, Option<T>) -> Drain
SparseStatesLog len         push_drain_past(max_len, impl FnOnce(LogMut) -> U) -> Drain
                            try_push_drain_past(max_len, impl FnOnce(LogMut) -> U) -> Result<Drain, AmountErr>
FramedStateLog  frame       push_drain_past(meta, T) -> Drain
FramedStatesLog frame       push_drain_past(meta, impl FnOnce(LogMut) -> U) -> Drain
                            try_push_drain_past(meta, impl FnOnce(LogMut) -> U) -> Result<Drain, AmountErr>

// idea: example that shows behavior of all log variants, transition variants are missing yet, rewrite to be a system
fn state_update(
    value: i32,
    meta: &RevMeta,
    mut condition_log: Local<SparseTransitionLog<()>>,

    mut dense_state: Local<DenseStateLog<i32>>,
    mut sparse_state: Local<SparseStateLog<i32>>,
    mut framed_state: Local<FramedStateLog<i32>>,

    mut dense_xor: Local<DenseTransitionLog<i32>>,
    mut sparse_xor: Local<SparseTransitionLog<i32>>,
    mut framed_xor: Local<FramedTransitionLog<i32>>,
) {
    let past_len = meta.past_log_len() as usize;

    match meta.direction() {
        RevDirection::NOT_LOG => if condition {
            dense.push_drain_past(past_len, value);
            sparse.push_drain_past(past_len, Some(value));
            framed.push_drain_past(meta, value);
        } else {
            let previous_value = *dense;
            dense.push_drain_past(past_len, previous_value);
            sparse.push_drain_past(past_len, None);
        },
        RevDirection::FORWARD_LOG => if condition {
            assert_eq!(dense.forward_log(), Ok(()));
            assert_eq!(sparse.forward_log(), Ok(true));
            assert_eq!(framed.forward_log()), Ok(()));
        } else {
            assert_eq!(dense.forward_log()), Ok(()));
            assert_eq!(sparse.forward_log(), Ok(false));
        },
        RevDirection::BackwardLog => if condition {
            assert_eq!(dense.backward_log()), Ok(()));
            assert_eq!(sparse.backward_log(), Ok(true));
            assert_eq!(framed.checked_backward_log(meta.present_world_state())), Ok(()));
        } else {
            assert_eq!(dense.backward_log()), Ok(()));
            assert_eq!(sparse.backward_log(), Ok(false));
        }
    }

    assert_eq!(*dense, *sparse);
    assert_eq!(*sparse, *framed);
}

if vec_deque::Drain could be iterated twice, U could be returned as well at *StatesLog
does not need to, just iterate regularily and return a drain
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
        let log_start_half = match past_end.first_half() {
            true => 0,
            false => 1,
        };
        let meta = RevMeta {
            max_world_states: max_len,
            present,
            past_end,
            future_end,
            direction,
            queue: None,
            log_start_half,
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
        assert_eq!(meta.contains(RevFrame::checked_new(0)), false, "{meta:#?}");
        assert_eq!(meta.contains(RevFrame::checked_new(1)), true, "{meta:#?}");
        assert_eq!(meta.contains(RevFrame::checked_new(2)), true, "{meta:#?}");
        assert_eq!(meta.contains(RevFrame::checked_new(3)), true, "{meta:#?}");
        assert_eq!(meta.contains(RevFrame::checked_new(4)), true, "{meta:#?}");
        assert_eq!(meta.contains(RevFrame::checked_new(5)), true, "{meta:#?}");
        assert_eq!(meta.contains(RevFrame::checked_new(6)), false, "{meta:#?}");
    }

    #[test]
    fn past_contains_returns_expected() {
        let meta = arrange(None, 3, 1..=5, InternalDirection::Pause);
        assert_eq!(
            meta.past_contains(RevFrame::checked_new(0)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.past_contains(RevFrame::checked_new(1)),
            true,
            "{meta:#?}"
        );
        assert_eq!(
            meta.past_contains(RevFrame::checked_new(2)),
            true,
            "{meta:#?}"
        );
        assert_eq!(
            meta.past_contains(RevFrame::checked_new(3)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.past_contains(RevFrame::checked_new(4)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.past_contains(RevFrame::checked_new(5)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.past_contains(RevFrame::checked_new(6)),
            false,
            "{meta:#?}"
        );
    }

    #[test]
    fn future_contains_returns_expected() {
        let meta = arrange(None, 3, 1..=5, InternalDirection::Pause);
        assert_eq!(
            meta.future_contains(RevFrame::checked_new(0)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.future_contains(RevFrame::checked_new(1)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.future_contains(RevFrame::checked_new(2)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.future_contains(RevFrame::checked_new(3)),
            false,
            "{meta:#?}"
        );
        assert_eq!(
            meta.future_contains(RevFrame::checked_new(4)),
            true,
            "{meta:#?}"
        );
        assert_eq!(
            meta.future_contains(RevFrame::checked_new(5)),
            true,
            "{meta:#?}"
        );
        assert_eq!(
            meta.future_contains(RevFrame::checked_new(6)),
            false,
            "{meta:#?}"
        );
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
