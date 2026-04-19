//! Contains log variants and types around them that can be used in reversible systems.
//!
//! ## Log variants
//!
//! - [`TransitionLog`], for storing singular values to transition a state forward or backward.
//! - [`TransitionsLog`], for storing multiple values to transition a state forward or backward.
//! - [`UpdateLog`], for keeping track when, how often and with which `past_len` value the other
//!   logs need to update in cases these updates happen irregularily.
//!
//! ## Optimal log length
//!
//! All logs in an application can sum up to a large amount of data and it is undesired to store any
//! more transition data than what is really needed to cover the [global log range].
//!
//! The transition logs need a `past_len` value as a parameter in their [`forward_push`] and
//! [`forward_extend`] methods to determine how many past log entries they should keep to not go
//! [`OutOfLog`] at some point. Depending on how often the log is pushing log entries, the correct
//! source has to be used:
//!
//! | source                          | situation                                   |
//! | ------------------------------- | ------------------------------------------- |
//! | [`NotLog`]                      | the log updates at every frame exactly once |
//! | [`UpdateLog::forward_past_len`] | the log updates arbitrarily                 |
//! | [`NonZeroU64::MAX`]             | the log is allowed to have unlimited growth |
//!
//! [`UpdateLog`] is able to manage its length by reading the [global log range] from [`RevMeta`].
//!
//! Ideally, as few logs and as few updates as possible are required for the application to keep the
//! memory usage low.
//!
//! ## Missing updates
//!
//! Errornous user code can cause logs missing the frame they are supposed to update at. In the case
//! of [log directions] this can cause the continuity of the world state to break. For example, when
//! the world is mutated at a certain frame and a log entry is saved to undo and redo this mutation,
//! it must not be missed to do that.
//!
//! If the code where the log updates happen does not run, log methods have no chance to detect and
//! report that error.
//!
//! The mechanisms of this crate, like [reversible scheduling] or [reversible commands], make sure
//! this contract is fulfilled, also in regard in which order mutations happen in a frame.
//!
//! Still, user code can make this fail. And while the subframe ordering cannot be verified, it can
//! be detected when a [`UpdateLog`] did not update at the correct frame in the correct amount of
//! times.
//!
//! **For the following to be checked the `track_update_logs` feature must be used.**
//!
//! Whenever an `UpdateLog` is updated, the information about which closest past and future frames
//! it expects to be updated again is stored in [`RevMeta`]. If at these frames the log is not
//! updated, [`RevMeta::update`] will report that via the [`RevMetaUpdateErr::UpdateLogsMissed`]
//! error that contains a list of [`UpdateLogMissed`]. The [`UpdateLogMissed::index`] was logged at
//! the INFO level when it is set for an individual `UpdateLog`.
//!
//! When bevy's `track_location` cargo feature is active, [`UpdateLogMissed::last_update`] also
//! contains the location where the `UpdateLog` was updated the last time.
//!
//! Note however that when a [`RevQueue::ClearThenRunForward`]/[`RevQueue::ClearThenPause`] is
//! applied, all INFO logged `UpdateLog` indices until then become invalid. This event is logged at
//! the INFO level as well. Every `UpdateLog` that updates after that will get new ids which is then
//! logged again. This is important to know because indices get reused.
//!
//! Log clears happen not when the queue above is applied but at the first next update of each log
//! after that.
//!
//! ## Draining transitions
//!
//! Updating the transition logs with [`forward_push`] and [`forward_extend`] also returns draining
//! iterators. These can be used to clean up other resources the logs are tracking, such as
//! temporary entities holding additional transition data.
//!
//! [`forward_push`]: TransitionLog::forward_push
//! [`forward_extend`]: TransitionsLog::forward_extend
//! [`NonZeroU64::MAX`]: core::num::NonZeroU64::MAX
//! [`RevMeta`]: crate::meta::RevMeta
//! [`NotLog`]: crate::meta::NotLog
//! [`RevMeta::update`]: crate::meta::RevMeta::update
//! [global log range]: crate::meta::RevMeta::contains
//! [log directions]: crate::meta::RevDirection::is_log
//! [`RevQueue::ClearThenRunForward`]: crate::meta::RevQueue::ClearThenRunForward
//! [`RevQueue::ClearThenPause`]: crate::meta::RevQueue::ClearThenPause
//! [`RevMetaUpdateErr::UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [`UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [reversible scheduling]: crate::schedule::RevSchedule
//! [reversible commands]: crate::undo_redo::commands::RevCommands

use alloc::{
    boxed::Box,
    collections::{VecDeque, vec_deque::Drain},
};
use bevy_ecs::change_detection::MaybeLocation;
use core::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    iter::FusedIterator,
};

#[cfg(feature = "track_update_logs")]
pub(crate) use update::limit::UpdateLogLimits;

#[cfg(feature = "track_update_logs")]
pub use update::limit::UpdateLogMissed;

pub use update::UpdateLog;

pub use transition::{TransitionDrain, TransitionLog};
pub use transitions::{
    TransitionsDrain, TransitionsDrainIters, TransitionsLog, TransitionsLogIterMut,
    TransitionsLogUpdate,
};
mod transition;
mod transitions;
mod update;

#[cfg(test)]
mod test;

/// An error that may be returned by the `backward_log`/`forward_log` methods of
/// [`TransitionLog`]/[`TransitionsLog`] in case they already were at the end of their log before
/// the method call.
///
/// This error indicates the continuity of the global state was broken and should not be ignored.
/// Instead, consider to pair the log with an [`UpdateLog`] that prevents this error. If then there
/// still is an error, it will be because an update was missed. In that case activate the
/// `track_update_logs` feature to get more information at which frame the update was expected to
/// happen.
///
/// See the [module level documentation](crate::log) for more information.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog(pub MaybeLocation);

impl OutOfLog {
    /// Creates new error with location tracking, if enabled via bevy's `track_location` cargo
    /// feature.
    #[track_caller]
    pub fn caller() -> Self {
        Self(MaybeLocation::caller())
    }
}

impl Display for OutOfLog {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
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

impl Error for OutOfLog {}

/// Iterator that drains both past and future log entries that got
/// [out of log](crate::meta::RevMeta::contains).
pub struct DrainAll<'a, T> {
    drain: Drain<'a, T>,
    gap_range: GapRange,
    gap_buffer: &'a mut Box<[T]>,
}

impl<'a, T> DrainAll<'a, T> {
    /// Calling this will cause `log` to become empty.
    ///
    /// Calling this again will return an empty iterator.
    fn new(
        log: &'a mut VecDeque<T>,
        gap_range: &mut GapRange,
        gap_buffer: &'a mut Box<[T]>,
    ) -> Self {
        // this_range is basically the state before the iteration, and gap_range will be mutated
        // here to the state after iteration/drop
        let mut this_range = *gap_range;

        let drain;
        if gap_range.end == log.len() {
            // only a past to drain
            drain = log.drain(..gap_range.start);
            this_range.end = gap_range.start;
            gap_range.end -= gap_range.start;
            gap_range.start = 0;
        } else if gap_range.start == 0 {
            // only a future to drain
            drain = log.drain(gap_range.end..);
            this_range.start = 0;
            this_range.end = 0;
        } else {
            // full drain including log items that are not out-of-log, but those will be buffered in
            // gap_buffer for reinsertion at drop
            gap_range.start = 0;
            gap_range.end = 0;
            drain = log.drain(..);
        }

        Self {
            drain,
            gap_range: this_range,
            gap_buffer,
        }
    }
}

impl<T> Iterator for DrainAll<'_, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.gap_range.start > 0 {
            // pre buffering
            self.gap_range.start -= 1;
            self.gap_range.end -= 1;
        } else if self.gap_range.end > 0 {
            // buffer before advancing
            *self.gap_buffer = self.drain.by_ref().take(self.gap_range.end).collect();
            self.gap_range.end = 0;
            self.gap_range.start = 0;
        }
        self.drain.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<T> ExactSizeIterator for DrainAll<'_, T> {
    fn len(&self) -> usize {
        if self.gap_range.buffer_pending() {
            self.drain
                .len()
                .saturating_sub(self.gap_range.end - self.gap_range.start)
        } else {
            self.drain.len()
        }
    }
}

impl<T> FusedIterator for DrainAll<'_, T> {}

impl<T> Drop for DrainAll<'_, T> {
    fn drop(&mut self) {
        if self.gap_range.buffer_pending() {
            let start = self.gap_range.start;
            let len = self.gap_range.end - start;
            *self.gap_buffer = self.drain.by_ref().skip(start).take(len).collect();
        }
    }
}

/// Defines a range of log entries that should be kept. Behaves like [`Range`](core::ops::Range) but
/// comes with usecase-relevant methods.
#[derive(Clone, Copy, Debug)]
struct GapRange {
    start: usize,
    end: usize,
}

impl GapRange {
    fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
    fn new_clear(index: usize) -> Self {
        Self {
            start: index,
            end: index,
        }
    }
    fn is_clear(self) -> bool {
        self.start == self.end
    }
    fn buffer_pending(self) -> bool {
        !self.is_clear() && self.start > 0
    }
    fn take_drain_past_end(&mut self) -> usize {
        let end = self.start;
        self.end -= end;
        self.start = 0;
        end
    }
    fn drain_future_start(&self) -> usize {
        self.end
    }
}

// todo: use prepend https://github.com/rust-lang/rust/issues/146975
fn prepend<T>(deque: &mut VecDeque<T>, gap_buffer: &mut Box<[T]>) {
    let mut buffer = core::mem::take(gap_buffer).into_iter();
    if deque.is_empty() {
        return deque.extend(buffer);
    }
    match buffer.len() {
        0 => {}
        1 => {
            let entry = buffer.next().unwrap(); // len != 0 expects an item
            deque.push_front(entry);
        }
        len => {
            deque.extend(buffer);
            deque.rotate_right(len);
        }
    }
}
