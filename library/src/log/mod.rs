//! # Log variants
//!
//! This module contains the types around the three log variants:
//!
//! - [`TransitionLog`], for storing singular values to transition a state forward or backward.
//! - [`TransitionsLog`], for storing multiple values to transition a state forward or backward.
//! - [`UpdateLog`], for keeping track when, how often and with which `max_past_len` value the other
//!   logs need to update in cases these updates happen irregularily. Can also be used as an compact
//!   alternative to `TransitionLog<bool>`.
//!
//! # Optimal log length
//!
//! All logs in an application can sum up to a large amount of data and it is undesired to store any
//! more transition data than what is really needed to cover the [global log length].
//!
//! The transition logs need a `max_past_len` value as a parameter in their `push` and
//! `push_drain_past` methods to determine how many past log entries they should keep to not go
//! [`OutOfLog`] at some point. Depending on how often the log is pushing log entries, the correct
//! source has to be used:
//!
//! | source                                | situation                                             |
//! | ------------------------------------- | ----------------------------------------------------- |
//! | [`RevMeta::past_len`]                 | the log updates at every frame exactly once           |
//! | `RevMeta::past_len * const N`         | the log updates at every frame exactly `N` times      |
//! | [`UpdateLog::push_get_past_len`]      | the log updates arbitrarily                           |
//! | [`UpdateLog::push_many_get_past_len`] | the log updates in varying batches, maybe arbitrarily |
//! | `u64::MAX`                            | the log is allowed to have unlimited growth           |
//!
//! [`UpdateLog`] is able to manage its length from reading the [global log length] from
//! [`RevMeta`].
//!
//! Ideally, as few logs and as few updates as possible are required for the application.
//!
//! # Continuity
//!
//! It is important that the transition logs are updated at the correct frames. This is trivial if
//! they update at every frame exactly once. For other cases, refer to this table:
//!
//! | [`RevDirection`] | method                   | [`RevMeta::now`] |
//! | ---------------- | ------------------------ | ---------------- |
//! | [`NOT_LOG`]      | `push`/`push_drain_past` | `n`              |
//! | [`BackwardLog`]  | `backward_log`           | `n-1`            |
//! | [`FORWARD_LOG`]  | `forward_log`            | `n`              |
//!
//! If a log is updated multiple times per frame, then these amounts must match for these frames as
//! well.
//!
//! As this can become hard to manage, a `UpdateLog` can support these updates by tracking when
//! and how often transition logs need to update. That log also provides the correct value for the
//! `max_past_len` parameter of the pushing methods. See the type documentation of [`UpdateLog`]
//! for examples.
//!
//! ## Missing updates
//!
//! Errornous user code can cause logs missing the frame they are supposed to update at. In the case
//! of [log directions] this can cause the continuity of the world state to break. For example, when
//! at the frame `n` a component is added to an entity, then the component must be removed at frame
//! `n-1` again when going backward and added again at frame `n` when going forward in the log.
//!
//! If the code where the log updates happen does not run, no log methods have a chance to detect
//! and report that error.
//!
//! The mechanisms of this crate, like [reversible scheduling] or [reversible commands], make sure
//! this contract is fulfilled, also in regard in which order mutations happen in a frame.
//!
//! Still, user code can make this fail. And while the subframe ordering cannot be verified, it can
//! be detected when a [`UpdateLog`] did not update at the correct frame in the correct amount of
//! times.
//!
//! Whenever that log is updated, the information about which closest past and future frames it
//! expects to be updated again is stored in [`RevMeta`]. If at these frames the log is not updated,
//! [`RevMeta::update`] will report that via the [`RevMetaUpdateErr::UpdateLogsMissed`] error that
//! contains a list of [`UpdateLogMissed`]. [`UpdateLogMissed::id`] contains the
//! [id of the `UpdateLog` instance] that was missed.
//!
//! Whenever a [`UpdateLog`] [initializes the internal id], this id is logged at the INFO level with
//! the location in the code. This should help to identify the issue.
//!
//! When bevy's `track_location` cargo feature is active, [`UpdateLogMissed::last_update`] also
//! contains the location where the [`UpdateLog`] was updated the last time.
//!
//! Note however that when a [`RevQueue::Clear`] is applied, all ids until then become invalid. This
//! event is logged at the INFO level as well. Every `UpdateLog` that updates after that will get
//! new ids which is then logged again.
//!
//! ## Example
//!
//! A [`UpdateLog::push_get_past_len`] runs at [frame `42`](crate::meta::RevMeta::now) during
//! [`RevDirection::NOT_LOG`]. This is the first time this log updated. `UpdateLog` will then inform
//! [`RevMeta`] that there is no future frame it expects to run at during
//! [`RevDirection::FORWARD_LOG`] but expects to run at frame `41` when going backward.
//!
//! When then [`UpdateLog::backward_log`] of this specific log runs at `41` during
//! [`RevDirection::BackwardLog`], this gets updated: Now there is no other frame in the past it
//! expects to run at, however it expects to run at frame `42` during [`RevDirection::FORWARD_LOG`].
//!
//! If these updates during the [log directions] do not happen however, [`RevMeta`] will notice
//! that, which triggers the [`UpdateLogsMissed`] error right at the frame where the update was
//! missed. That way the `backward_log`/`forward_log` methods of transition logs that are updated
//! alongside will never fail with [`OutOfLog`].
//!
//!
//! [id of the `UpdateLog` instance]: UpdateLog::id
//! [initializes the internal id]: UpdateLog::pre_update
//! [`RevMeta`]: crate::meta::RevMeta
//! [`RevMeta::now`]: crate::meta::RevMeta::now
//! [`RevMeta::past_len`]: crate::meta::RevMeta::past_len
//! [`RevMeta::update`]: crate::meta::RevMeta::update
//! [global log length]: crate::meta::RevMeta::contains
//! [`RevDirection`]: crate::meta::RevDirection
//! [`RevDirection::NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
//! [`NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
//! [`RevDirection::BackwardLog`]: crate::meta::RevDirection::BackwardLog
//! [`BackwardLog`]: crate::meta::RevDirection::BackwardLog
//! [`RevDirection::FORWARD_LOG`]: crate::meta::RevDirection::FORWARD_LOG
//! [`FORWARD_LOG`]: crate::meta::RevDirection::FORWARD_LOG
//! [log directions]: crate::meta::RevDirection::is_log
//! [`RevQueue::Clear`]: crate::meta::RevQueue::Clear
//! [`RevMetaUpdateErr::UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [`UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [reversible scheduling]: crate::schedule::RevSchedule
//! [reversible commands]: crate::undo_redo::RevCommands

use bevy_ecs::change_detection::MaybeLocation;

pub(crate) use update::limits::{UpdateLogLimits, UpdateLogState};
pub use update::{UpdateLog, limits::UpdateLogMissed};

pub use transition::{
    TransitionDrainAll, TransitionDrainFuture, TransitionDrainPast, TransitionDrains, TransitionLog,
};

pub use transitions::{
    LogMut, TransitionsDrainAll, TransitionsDrainChunkable, TransitionsDrainFuture,
    TransitionsDrainPast, TransitionsDrains, TransitionsLog, TransitionsLogIterMut,
    TransitionsLogUpdate,
};

mod transition;
mod transitions;
mod update;

/// Defines in which way a log has to be adjusted to reflect new changes to
/// [`RevMeta`](crate::meta::RevMeta) since the last time the log was updated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PreUpdateKind {
    /// Keep the log unchanged
    Nothing,

    /// Remove log entries that are in the future
    RemoveFuture,

    /// Remove all log entries.
    RemoveLog,
}

/// An error that may be returned by the `backward_log`/`forward_log` methods of
/// [`TransitionLog`]/[`TransitionsLog`] in case they already were at the end of their log before
/// the method call.
///
/// This error indicates the continuity of the global state was broken.
///
/// See the [module level documentation](crate::log) for more information.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog(MaybeLocation);

impl OutOfLog {
    /// Constructor with location tracking, if enabled.
    #[track_caller]
    fn new() -> Self {
        Self(MaybeLocation::caller())
    }
}

impl core::fmt::Display for OutOfLog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "a `Transition(s)Log` was attempted to be traversed beyond its bounds"
        )?;
        match self.0.into_option() {
            Some(location) => write!(f, " at {location}"),
            None => write!(
                f,
                ", use bevy's `track_location` cargo feature for the location in code"
            ),
        }
    }
}

impl core::error::Error for OutOfLog {}
