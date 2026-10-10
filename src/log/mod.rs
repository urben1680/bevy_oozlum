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
//! Note however that when a [`RevQueue::ClearThenRunNotLog`]/[`RevQueue::ClearThenPause`] is
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
//! [`RevQueue::ClearThenRunNotLog`]: crate::meta::RevQueue::ClearThenRunNotLog
//! [`RevQueue::ClearThenPause`]: crate::meta::RevQueue::ClearThenPause
//! [`RevMetaUpdateErr::UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [`UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [reversible scheduling]: crate::schedule::RevSchedule
//! [reversible commands]: crate::undo_redo::commands::RevCommands

use alloc::collections::VecDeque;
use bevy_ecs::change_detection::MaybeLocation;
use core::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    iter::FusedIterator,
    mem::take,
};
use nonmax::NonMaxUsize;

#[cfg(feature = "track_update_logs")]
pub(crate) use update::limit::UpdateLogLimits;

#[cfg(feature = "track_update_logs")]
pub use update::limit::UpdateLogMissed;

pub use update::{UpdateLog, UpdateMissedAt};

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
/// This error indicated an invalid world state.
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
    deque: &'a mut VecDeque<T>,
    gap_range: GapRange,
}

impl<T> Iterator for DrainAll<'_, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.gap_range.is_clear() {
            return self.deque.pop_front();
        }
        if let Some(to_drain_past) = self.gap_range.drain_past_len {
            let to_drain_past = to_drain_past.get();
            if to_drain_past > 0 {
                self.gap_range.drain_past_len = NonMaxUsize::new(to_drain_past - 1);
                return self.deque.pop_front();
            } else if self.gap_range.drain_future_len == 0 {
                return None;
            }
            self.gap_range.drain_past_len = None;
            self.deque.rotate_left(self.gap_range.keep_len);
        } else if self.gap_range.drain_future_len == 0 {
            return None;
        }
        self.gap_range.drain_future_len -= 1;
        self.deque.pop_front()
    }

    fn for_each<F>(mut self, mut f: F)
    where
        Self: Sized,
        F: FnMut(Self::Item),
    {
        if self.gap_range.is_clear() {
            self.deque.drain(..).for_each(f);
        } else if let Some(to_drain_past) = self.gap_range.drain_past_len {
            self.deque.drain(..to_drain_past.get()).for_each(&mut f);
            self.deque.drain(self.gap_range.keep_len..).for_each(f);
        } else {
            self.deque
                .drain(..self.gap_range.drain_future_len)
                .for_each(f);
        }
        self.gap_range.take_all();
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<T> ExactSizeIterator for DrainAll<'_, T> {
    fn len(&self) -> usize {
        self.deque.len() - self.gap_range.keep_len
    }
}

impl<T> FusedIterator for DrainAll<'_, T> {}

impl<T> Drop for DrainAll<'_, T> {
    fn drop(&mut self) {
        if self.gap_range.is_clear() {
            self.deque.clear();
        } else if let Some(to_drain_past) = self.gap_range.drain_past_len {
            // todo: use retain_back when MSRV of bevy is 1.99
            self.deque.drain(..to_drain_past.get());
            self.deque.truncate(self.gap_range.keep_len);
        } else {
            // todo: use retain_back when MSRV of bevy is 1.99
            self.deque.drain(..self.gap_range.drain_future_len);
        }
    }
}

/// Defines lengthes of drainable segments of a `VecDeque`.
#[derive(Clone, Copy, Debug)]
struct GapRange {
    /// Is `None` if deque was rotated
    drain_past_len: Option<NonMaxUsize>,
    drain_future_len: usize,
    keep_len: usize,
}

impl GapRange {
    fn is_clear(self) -> bool {
        self.keep_len == 0
    }
    fn drain_past_len_or_zero(self) -> usize {
        self.drain_past_len.unwrap_or(NonMaxUsize::ZERO).get()
    }
    fn take_drain_past_len(&mut self) -> usize {
        self.drain_past_len
            .as_mut()
            .map(take)
            .unwrap_or(NonMaxUsize::ZERO)
            .get()
    }
    fn take_drain_future_len(&mut self) -> usize {
        take(&mut self.drain_future_len)
    }
    fn take_all(&mut self) -> Self {
        let to_drain_past = self.drain_past_len.as_mut().map(take);
        let to_drain_future = take(&mut self.drain_future_len);
        Self {
            drain_past_len: to_drain_past,
            drain_future_len: to_drain_future,
            keep_len: self.keep_len,
        }
    }
}
