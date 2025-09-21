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

pub(crate) use past_len::limits::{PastLenLogLimits, PastLenState};
pub use past_len::{PastLenLog, limits::PastLenLogMissed};

pub use transition::{
    TransitionDrain, TransitionDrainFuture, TransitionDrainPast, TransitionDrains, TransitionLog,
};

pub use transitions::{
    LogMut, TransitionLogUpdateMut, TransitionsDrain, TransitionsDrainChunkable,
    TransitionsDrainFuture, TransitionsDrainPast, TransitionsDrains, TransitionsLog,
    TransitionsLogUpdate,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PreUpdateVariant {
    Nothing,
    RemoveFuture,
    RemoveLog,
}

/// An error that may be returned by the `backward_log`/`forward_log` methods of
/// [`TransitionLog`]/[`TransitionsLog`] in case they already were at the end of their log before
/// the method call.
#[derive(Clone, Copy, core::fmt::Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog;

impl core::fmt::Display for OutOfLog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "a `Transition(s)Log` was attempted to be traversed beyond its bounds"
        )
    }
}

impl core::error::Error for OutOfLog {}
