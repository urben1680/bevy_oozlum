//! This module contains the types around the three log variants:
//!
//! - [`TransitionLog`], for storing singular values to transition a state forward or backward.
//! - [`TransitionsLog`], for storing multiple values to transition a state forward or backward.
//! - [`PastLenLog`], for keeping track which `max_past_len` value the other logs need to be passed
//!   to in cases these are updated irregularily and
//!   [`RevMeta::past_len`](crate::meta::RevMeta::past_len) is not applicable for them.
//!
//! Each log type contains further documentation and examples.

use bevy::ecs::change_detection::MaybeLocation;

pub(crate) use past_len::limits::{PastLenLogLimits, PastLenState};
pub use past_len::{PastLenLog, limits::PastLenLogMissed};

pub use transition::{
    TransitionDrainAll, TransitionDrainFuture, TransitionDrainPast, TransitionDrains, TransitionLog,
};

pub use transitions::{
    LogMut, TransitionsDrainAll, TransitionsDrainChunkable, TransitionsDrainFuture,
    TransitionsDrainPast, TransitionsDrains, TransitionsLog, TransitionsLogIterMut,
    TransitionsLogUpdate,
};

mod past_len;
mod transition;
mod transitions;

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
/// This error cannot occur if:
/// 1. The `max_past_len` parameter in their `push_and_truncate_past`/ `push_and_drain_past` methods
///    is always taken from the situational correct source:
///    - [`RevMeta::past_len`](crate::meta::RevMeta::past_len) (log is updated every frame)
///    - [`PastLenLog::update_get`] (log is updated arbitrarily)
///    - [`PastLenLog::update_many_get`] (log is updated in varying batches)
///    - `u64::MAX` (log is allowed to have unlimited growth)
/// 2. No [log updates](crate::meta::RevDirection::is_log) are missed that correspond to the frames
///    the log was updated at during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
///    This is trivial if the log simply updates every frame. In other cases, this can be tracked
///    with a [`PastLenLog`] that is updated along the log. This causes
///    [`RevMeta::update`](crate::meta::RevMeta::update) to return
///    [`RevMetaUpdateErr::PastLenLogsMissed`](crate::meta::RevMetaUpdateErr::PastLenLogsMissed)
///    before `OutOfLog` could be encountered. The specific `PastLenLog` from the error can be
///    identified in two ways:
///    - When [`PastLenLog::pre_update`] sets it's [id](PastLenLog::id), an info log will be written
///      which can be compared to  [`PastLenLogMissed::id`] from the error above. Note that the id
///      will change when [`RevQueue::Clear`](crate::meta::RevQueue::Clear) is applied, which is
///      also logged.
///    - Using bevy's `track_location` cargo feature to read [`PastLenLogMissed::last_update`].
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
