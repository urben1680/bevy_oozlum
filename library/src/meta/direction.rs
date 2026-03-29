use alloc::format;
use bevy_ecs::{
    change_detection::Tick,
    component::ComponentId,
    error::Result as BevyResult,
    query::FilteredAccessSet,
    system::{Command, ReadOnlySystemParam, SystemMeta, SystemParam, SystemParamValidationError},
    world::{World, unsafe_world_cell::UnsafeWorldCell},
};
use core::{fmt::Display, num::NonZeroU64};

use crate::meta::RevMeta;

/// The direction [`RevUpdate`](crate::schedule::RevUpdate) is currently running at. Reversible
/// systems should mind this value.
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

    /// Is [`NotLog`] or [`ForwardLog`].
    ///
    /// [`NotLog`]: Self::NotLog
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

    /// Is [`NotLog`].
    ///
    /// [`NotLog`]: Self::NotLog
    pub fn is_not_log(self) -> bool {
        matches!(self, Self::NotLog(_))
    }

    /// Returns the [`NotLog`] contained in [`RevDirection::NotLog`].
    ///
    /// # Panics
    ///
    /// This method panics for different directions.
    pub fn past_len(self) -> NotLog {
        self.get_past_len().unwrap()
    }

    /// Returns the [`NotLog`] contained in [`RevDirection::NotLog`].
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
pub(super) enum RunningOrRan {
    Running(RevDirection),
    Ran(RevDirection),
    Pause { after_log: bool },
}

/// The next state [`RevMeta`] should be in via [`RevMeta::set_queue`], will be applied when
/// [`run_rev_update`] runs. Before that, a different queue can be set, which will
/// overwrite a different pending value. Can also be [unset] before that.
/// 
/// This type may also be used as a [`Command`] that can fail if [`RevMeta`] is missing.
///
/// [`run_rev_update`]: super::run_rev_update
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

/// A type for when [`RevUpdate`] is currently running at [`RevDirection::NotLog`]. This is used as
/// an argument for:
/// 
/// - [`RevCommands`]
/// - [`RevEntityCommands`]
/// - [`BuffersUndoRedo`]
/// 
/// ... to ensure these are only queued at that direction. Because of this, this type must not be
/// stored past the frame it is accessed from.
/// 
/// It is also a newtyped value of [`RevMeta::past_len`] which, during this direction, can never be
/// zero. As this, [`TransitionLog::forward_push`]/[`TransitionsLog::forward_extend`] can use it for
/// their `past_len` argument. This is adviced when the logs are updated exactly once per reversible
/// frame. For every other update strategy, use the value returned by
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
pub struct NotLog(pub(super) NonZeroU64);

impl From<NotLog> for NonZeroU64 {
    fn from(value: NotLog) -> Self {
        value.0
    }
}

// SAFETY: registers RevMeta read access and reads nothing else
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
                "RevMeta requested by {} does not exist or RevUpdate is not running or is running in log",
                system_meta.name()
            );
        })
    }
}

// SAFETY: NotLog only reads RevMeta resource
unsafe impl ReadOnlySystemParam for NotLog {}
