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

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog;

impl Display for OutOfLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a log was traversed beyond its bounds")
    }
}

impl Error for OutOfLog {}

const INDEX_OOB: &'static str = "self.index should always be <= the deque len, so successfully reducing \
    it without underflow is expected to result in a valid index into the log but this is not the case here, \
    the log was in an invalid state before calling the current method, this is a crate bug or the log was \
    deserialized with invalid data";
