//! This module contains the types around the three log variants:
//!
//! - [`TransitionLog`], for storing singular values to transition a state forward or backward.
//! - [`TransitionsLog`], for storing multiple values to transition a state forward or backward.
//! - [`PastLenLog`], for keeping track which `max_past_len` value the other logs need to be passed
//!   to in cases these are updated irregularily and
//!   [`RevMeta::past_len`](crate::meta::RevMeta::past_len) is not applicable for them.
//!
//! Each log type contains further documentation and examples.

mod past_len;
mod transition;
mod transitions;

use bevy::ecs::change_detection::MaybeLocation;
pub(crate) use past_len::limits::{PastLenLogLimits, PastLenState};
pub use past_len::{PastLenLog, limits::PastLenLogMissed};

pub use transition::{
    TransitionDrainAll, TransitionDrainFuture, TransitionDrainPast, TransitionDrains, TransitionLog,
};

pub use transitions::{
    LogMut, TransitionLogUpdateMut, TransitionsDrainAll, TransitionsDrainChunkable,
    TransitionsDrainFuture, TransitionsDrainPast, TransitionsDrains, TransitionsLog,
    TransitionsLogUpdate,
};

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
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog(MaybeLocation);

impl OutOfLog {
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
