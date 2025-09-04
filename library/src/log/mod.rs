use std::{
    error::Error,
    fmt::{Debug, Display},
};

mod past_len;
mod transition;
mod transitions;

pub use past_len::PastLenLog;
pub(crate) use past_len::limits::{PastLenLogLimits, PastLenLogMissed, PastLenState};
pub use transition::TransitionLog;
pub use transitions::{EntryAmount, LogMut, TransitionsLog, ValueEntry};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PreUpdateVariant {
    Nothing,
    DropFuture,
    DropLog,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog; // todo: add location

impl Display for OutOfLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a log was traversed beyond it's bounds or it was attempted to queue RevMeta to a frame outside the log"
        )
    }
}

impl Error for OutOfLog {}

const INDEX_OOB: &'static str = "self.index should always be <= the deque len, so successfully reducing \
    it without underflow is expected to result in a valid index into the log but this is not the case here, \
    the log was in an invalid state before calling the current method, this is a crate bug or the log was \
    deserialized with invalid data";
