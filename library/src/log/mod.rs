//! This module contains the types around the three log variants:
//!
//! - [`TransitionLog`], for storing singular values to transition a state forward or backward
//! - [`TransitionsLog`], for storing a number of values to transition a state forward or backward
//! - [`PastLenLog`], for keeping track which `max_past_len` value the other logs need to be passed
//!   to in cases these are updated irregularily and
//!   [`RevMeta::past_len`](crate::meta::RevMeta::past_len) is not applicable for them.
//!
//! Each log type contains further documentation and examples.

use std::{
    error::Error,
    fmt::{Debug, Display},
};

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
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog;

impl Display for OutOfLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a `Transition(s)Log` was traversed beyond its bounds")
    }
}

impl Error for OutOfLog {}
