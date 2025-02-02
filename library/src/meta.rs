use core::num::NonZeroU64;
use std::{error::Error, fmt::Display};

use bevy::{
    ecs::{
        archetype::ArchetypeComponentId,
        change_detection::Mut,
        component::{ComponentId, Tick},
        query::Access,
        system::{IntoSystem, ReadOnlySystemParam, Res, Resource, System, SystemMeta, SystemParam},
        world::{unsafe_world_cell::UnsafeWorldCell, World},
    },
    log::error_once,
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::info,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::{log::OutOfLog, schedule::RevUpdate, undo_redo::UndoRedoBuffer};

#[derive(Clone, Debug, PartialEq)]
pub enum TryRunRevUpdateError {
    RevMetaMissingFirstCall,
    RevMetaMissing { existed_previously: bool },
    RevMetaRemovedInSchedule { frame: Option<u64> },
    UnexpectedInitialRunning(RevMeta),
    RevUpdateMissing(RevMeta),
    UndoRedoBufferMissingAfterUpdate(RevMeta),
    UndoRedoBufferNotEmptyBeforeUpdate(RevMeta),
    UndoRedoBufferOutOfLogAfterUpdate(RevMeta),
}

impl Display for TryRunRevUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RevMetaMissingFirstCall => write!(
                f,
                "RevMeta does not exist yet, RevUpdate will not be called until it is inserted"
            ),
            Self::RevMetaMissing { .. } => write!(
                f,
                "RevMeta was removed, RevUpdate will not be called until it is inserted again"
            ),
            Self::RevMetaRemovedInSchedule { frame } => write!(
                f,
                "RevMeta was removed in while RevUpdate ran at frame {}",
                frame.unwrap_or(u64::MAX)
            ),
            Self::UnexpectedInitialRunning(meta) => write!(
                f,
                "RevMeta was in a running state at frame {} before RevUpdate could run",
                meta.now()
            ),
            Self::RevUpdateMissing(meta) => {
                write!(f, "RevUpdate was missing at frame {}", meta.now())
            }
            Self::UndoRedoBufferMissingAfterUpdate(meta) => write!(
                f,
                "UndoRedoBuffer was removed during RevUpdate at frame {}",
                meta.now()
            ),
            Self::UndoRedoBufferNotEmptyBeforeUpdate(meta) => write!(
                f,
                "UndoRedoBuffer was not fully drained at frame {}, RevUpdate is not run",
                meta.now()
            ),
            Self::UndoRedoBufferOutOfLogAfterUpdate(meta) => write!(
                f,
                "UndoRedoBuffer was in an invalid state after RevUpdate ran at frame {}",
                meta.now()
            ),
        }
    }
}

impl Error for TryRunRevUpdateError {}

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
    RunningForwardLog { updates_until_pause: NonZeroU64 },
    RunningBackwardLog { updates_until_pause: NonZeroU64 },
    RanForward,
    RanForwardLog { updates_until_pause: NonZeroU64 },
    RanBackwardLog { updates_until_pause: NonZeroU64 },
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
    pub max_world_states: Option<NonZeroU64>,
    past_end: u64,
    now: u64,
    future_end: u64,
    /// If Some, is either a Running* variant or Pause
    queue: Option<InternalDirection>,
    direction: InternalDirection,
}

impl Default for RevMeta {
    fn default() -> Self {
        Self::new(Some(NonZeroU64::MIN), 0, false)
    }
}

impl RevMeta {
    pub const fn new(max_world_states: Option<NonZeroU64>, now: u64, paused: bool) -> Self {
        Self {
            max_world_states,
            now,
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
    pub fn future_end(&self) -> u64 {
        self.future_end
    }
    pub const fn now(&self) -> u64 {
        self.now
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
    // todo: no longer needed to have that many options, simplify
    pub fn past_contains(&self, frame: u64) -> bool {
        self.now.wrapping_sub(frame).wrapping_sub(1) < (self.now - self.past_end)
    }
    // todo: no longer needed to have that many options, simplify
    pub fn future_contains(&self, frame: u64) -> bool {
        self.future_end.wrapping_sub(frame) < (self.future_end - self.now)
    }
    pub fn clear(&mut self) {
        self.past_end = self.now;
        self.future_end = self.now;
    }
    /// Queue to go forward.
    ///
    /// Will cause logged future frames to be forgotten.
    pub fn queue_not_log_forward(&mut self) {
        self.queue = Some(InternalDirection::RunningForward);
    }
    pub fn queue_log(&mut self, to: u64) -> Result<u64, OutOfLog> {
        let to_past = self.now.wrapping_sub(to);
        let to_future = to.wrapping_sub(self.now);

        if to_past <= self.past_len() {
            self.queue = Some(match NonZeroU64::new(to_past) {
                Some(updates_until_pause) => InternalDirection::RunningBackwardLog {
                    updates_until_pause,
                },
                None => InternalDirection::Pause,
            });
            Ok(to_past)
        } else if to_future <= self.future_len() {
            self.queue = Some(match NonZeroU64::new(to_future) {
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
    pub fn try_run_rev_update(world: &mut World) -> Result<(), TryRunRevUpdateError> {
        #[derive(Resource, Clone, Copy)]
        struct Existed(bool);

        if world.contains_resource::<Self>() {
            world.insert_resource(Existed(true));
        } else {
            let err = match world.get_resource::<Existed>().cloned() {
                None => TryRunRevUpdateError::RevMetaMissingFirstCall,
                Some(Existed(existed_previously)) => {
                    TryRunRevUpdateError::RevMetaMissing { existed_previously }
                }
            };
            world.insert_resource(Existed(false));
            return Err(err);
        }

        world.resource_scope(|world: &mut World, mut meta: Mut<Self>| {
            let buffer = world.get_resource_or_init::<UndoRedoBuffer>();

            if !buffer.undo_redo_is_empty() {
                return Err(TryRunRevUpdateError::UndoRedoBufferNotEmptyBeforeUpdate(
                    meta.clone(),
                ));
            }

            if meta.get_direction().is_some() {
                return Err(TryRunRevUpdateError::UnexpectedInitialRunning(meta.clone()));
            }
            let previous = meta.clone();
            let result = meta.update(|meta| {
                let frame = meta.now();
                let result = world.try_schedule_scope(RevUpdate, |world, schedule| {
                    world.insert_resource(meta.clone());
                    schedule.run(world);
                });
                match result {
                    Ok(()) => {
                        if !world.contains_resource::<UndoRedoBuffer>() {
                            Err(TryRunRevUpdateError::UndoRedoBufferMissingAfterUpdate(
                                meta.clone(),
                            ))
                        } else {
                            world.resource_scope(|world, mut buffer: Mut<UndoRedoBuffer>| {
                                match buffer.update_finalize(&meta, world) {
                                    Ok(()) => Ok(frame),
                                    Err(OutOfLog) => Err(
                                        TryRunRevUpdateError::UndoRedoBufferOutOfLogAfterUpdate(
                                            meta.clone(),
                                        ),
                                    ),
                                }
                            })
                        }
                    }
                    Err(_) => Err(TryRunRevUpdateError::RevUpdateMissing(meta.clone())),
                }
            });

            match result.transpose() {
                Ok(frame) => {
                    let Some(updated) = world.remove_resource::<Self>() else {
                        return Err(TryRunRevUpdateError::RevMetaRemovedInSchedule { frame });
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
    pub fn run_rev_update(world: &mut World) {
        match Self::try_run_rev_update(world) {
            Err(TryRunRevUpdateError::RevMetaMissingFirstCall) => info!(
                "RevMeta does not exist yet, reversible schedule RevUpdate will not be called until it is inserted"
            ),
            Err(TryRunRevUpdateError::RevMetaMissing { existed_previously, .. }) => if existed_previously {
                info!(
                    "RevMeta was removed, reversible schedule RevUpdate will not be called until it is inserted again"
                )
            },
            Err(err) => error_once!("{err}"),
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
    pub fn update<Out>(&mut self, c: impl FnOnce(&mut Self) -> Out) -> Option<Out> {
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
        fn reduction_successful(updates_until_pause: &mut NonZeroU64) -> bool {
            NonZeroU64::new(updates_until_pause.get() - 1)
                .map(|reduced| *updates_until_pause = reduced)
                .is_some()
        }

        match self.queue.take() {
            Some(queue) => {
                self.direction = queue;
                self.now = match self.get_direction() {
                    Some(RevDirection::NOT_LOG) => return self.update_forward(),
                    Some(RevDirection::FORWARD_LOG) => self.now + 1,
                    Some(RevDirection::BackwardLog) => self.now - 1,
                    None => self.now,
                };
            }
            None => {
                self.direction.start_running();
                let updated_at_log = match &mut self.direction {
                    InternalDirection::RunningForward => return self.update_forward(),
                    InternalDirection::RunningForwardLog {
                        updates_until_pause,
                    } => reduction_successful(updates_until_pause)
                        .then(|| self.now + 1),
                    InternalDirection::RunningBackwardLog {
                        updates_until_pause,
                    } => reduction_successful(updates_until_pause)
                        .then(|| self.now - 1),
                    _ /* Pause */ => None,
                };
                match updated_at_log {
                    Some(updated) => self.now = updated,
                    None => self.direction = InternalDirection::Pause,
                }
            }
        }
    }
    fn update_forward(&mut self) {
        self.now += 1;
        self.future_end = self.now;
        if let Some(max_world_states) = self.max_world_states.map(NonZeroU64::get) {
            // past states equal to max states is too many as the present state has to be added to the comparision
            if self.past_len() >= max_world_states {
                self.past_end = self.now + 1 - max_world_states;
            }
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

#[cfg(test)]
mod test {
    use std::ops::RangeInclusive;

    use super::*;

    const ONE: NonZeroU64 = NonZeroU64::MIN;
    const TWO: NonZeroU64 = unsafe { NonZeroU64::new_unchecked(2) };
    const THREE: NonZeroU64 = unsafe { NonZeroU64::new_unchecked(3) };

    /// Constructs [`RevMeta`] and asserts the values are valid
    fn arrange(
        max_len: Option<NonZeroU64>,
        present: u64,
        range: RangeInclusive<u64>,
        direction: InternalDirection,
    ) -> RevMeta {
        let past_end = *range.start();
        let future_end = *range.end();
        let meta = RevMeta {
            max_world_states: max_len,
            now: present,
            past_end,
            future_end,
            direction,
            queue: None,
        };
        assert!(*range.start() <= present, "{meta:?}");
        match direction {
            InternalDirection::RunningForward => assert_eq!(present, range.end() + 2, "{meta:?}"),
            InternalDirection::RunningForwardLog {
                updates_until_pause,
            } => {
                assert!(present <= *range.end(), "{meta:?}");
                assert!(
                    present + updates_until_pause.get() as u64 - 1 <= *range.end(),
                    "{meta:?}"
                );
            }
            InternalDirection::RunningBackwardLog {
                updates_until_pause,
            } => {
                assert!(present <= *range.end(), "{meta:?}");
                assert!(
                    range.start() + updates_until_pause.get() as u64 - 1 <= present,
                    "{meta:?}"
                );
            }
            InternalDirection::Pause => assert!(present <= *range.end(), "{meta:?}"),
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
        assert_eq!(meta.now, 1);

        meta.update_internal();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.now, 1);
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
        assert_eq!(meta.now, 0);

        meta.update_internal();
        assert_eq!(meta.get_direction(), None);
        assert_eq!(meta.now, 0);
    }

    #[test]
    fn start_grows_according_to_max_len() {
        let mut meta = RevMeta::new(Some(TWO), 0, false);

        meta.update_internal();
        assert_eq!(meta.now, 1);
        assert_eq!(meta.len(), 2);

        meta.update_internal();
        assert_eq!(meta.now, 2);
        assert_eq!(meta.len(), 2);
    }

    #[test]
    fn queue_log_to_out_of_range_fails() {
        let mut meta = arrange(None, 2, 1..=3, InternalDirection::Pause);

        assert_eq!(meta.queue_log(0), Err(OutOfLog));
        assert_eq!(meta.queue_log(4), Err(OutOfLog));
    }

    #[test]
    fn queue_log_to_in_range_succeeds() {
        let mut meta = arrange(None, 2, 1..=3, InternalDirection::Pause);

        assert_eq!(meta.queue_log(1), Ok(1));
        assert_eq!(meta.queue_log(3), Ok(1));
    }

    #[test]
    fn queue_log_to_present_pauses() {
        let mut meta = arrange(
            None,
            2,
            1..=3,
            InternalDirection::RunningForwardLog {
                updates_until_pause: NonZeroU64::new(2).unwrap(),
            },
        );
        assert_eq!(meta.queue_log(2), Ok(0));
        meta.update_internal();
        assert!(meta.paused());
    }

    #[test]
    fn contains_returns_expected() {
        let meta = arrange(None, 3, 1..=5, InternalDirection::Pause);
        assert_eq!(meta.contains(0), false, "{meta:#?}");
        assert_eq!(meta.contains(1), true, "{meta:#?}");
        assert_eq!(meta.contains(2), true, "{meta:#?}");
        assert_eq!(meta.contains(3), true, "{meta:#?}");
        assert_eq!(meta.contains(4), true, "{meta:#?}");
        assert_eq!(meta.contains(5), true, "{meta:#?}");
        assert_eq!(meta.contains(6), false, "{meta:#?}");
    }

    #[test]
    fn past_contains_returns_expected() {
        let meta = arrange(None, 3, 1..=5, InternalDirection::Pause);
        assert_eq!(meta.past_contains(0), false, "{meta:#?}");
        assert_eq!(meta.past_contains(1), true, "{meta:#?}");
        assert_eq!(meta.past_contains(2), true, "{meta:#?}");
        assert_eq!(meta.past_contains(3), false, "{meta:#?}");
        assert_eq!(meta.past_contains(4), false, "{meta:#?}");
        assert_eq!(meta.past_contains(5), false, "{meta:#?}");
        assert_eq!(meta.past_contains(6), false, "{meta:#?}");
    }

    #[test]
    fn future_contains_returns_expected() {
        let meta = arrange(None, 3, 1..=5, InternalDirection::Pause);
        assert_eq!(meta.future_contains(0), false, "{meta:#?}");
        assert_eq!(meta.future_contains(1), false, "{meta:#?}");
        assert_eq!(meta.future_contains(2), false, "{meta:#?}");
        assert_eq!(meta.future_contains(3), false, "{meta:#?}");
        assert_eq!(meta.future_contains(4), true, "{meta:#?}");
        assert_eq!(meta.future_contains(5), true, "{meta:#?}");
        assert_eq!(meta.future_contains(6), false, "{meta:#?}");
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
        assert_eq!(meta.len(), 3, "{meta:#?}");
        meta.queue_not_log_forward();
        meta.update_internal();
        assert_eq!(meta.len(), 2, "{meta:#?}");
    }
}
