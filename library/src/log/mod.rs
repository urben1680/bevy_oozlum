//! This module contains log variants and types around them that can be used in reversible systems.
//!
//! # Log variants
//!
//! - [`TransitionLog`], for storing singular values to transition a state forward or backward.
//! - [`TransitionsLog`], for storing multiple values to transition a state forward or backward.
//! - [`UpdateLog`], for keeping track when, how often and with which `past_len` value the other
//!   logs need to update in cases these updates happen irregularily.
//!
//! # Optimal log length
//!
//! All logs in an application can sum up to a large amount of data and it is undesired to store any
//! more transition data than what is really needed to cover the [global log length].
//!
//! The transition logs need a `past_len` value as a parameter in their [`forward_push`] and
//! [`forward_extend`] methods to determine how many past log entries they should keep to not go
//! [`OutOfLog`] at some point. Depending on how often the log is pushing log entries, the correct
//! source has to be used:
//!
//! | source                          | situation                                   |
//! | ------------------------------- | ------------------------------------------- |
//! | [`NotLog`]                 | the log updates at every frame exactly once |
//! | [`UpdateLog::forward_past_len`] | the log updates arbitrarily                 |
//! | [`NonZeroU64::MAX`]             | the log is allowed to have unlimited growth |
//!
//! [`UpdateLog`] is able to manage its length by reading the [global log range] from [`RevMeta`].
//!
//! Ideally, as few logs and as few updates as possible are required for the application to keep the
//! memory usage low.
//!
//! # Continuity
//!
//! It is important that the transition logs are updated at the correct frames. This is trivial if
//! they update at every frame exactly once. For other cases, refer to this table:
//!
//! | [`RevDirection`] | method                          | [`RevMeta::now`] |
//! | ---------------- | ------------------------------- | ---------------- |
//! | [`Forward`]      | `forward_push`/`forward_extend` | `n`              |
//! | [`BackwardLog`]  | `backward_log`                  | `n-1`            |
//! | [`ForwardLog`]   | `forward_log`                   | `n`              |
//!
//! If a log is updated multiple times per frame, then these amounts must match for these frames as
//! well.
//!
//! As this can become hard to manage, a [`UpdateLog`] can support these updates by tracking when
//! and how often transition logs need to update. See the type documentation of [`UpdateLog`] for
//! examples.
//!
//! ## Missing updates
//!
//! Errornous user code can cause logs missing the frame they are supposed to update at. In the case
//! of [log directions] this can cause the continuity of the world state to break. For example, when
//! at the frame `n` a component is added to an entity, then the component must be removed at frame
//! `n-1` again when going backward and added again at frame `n` when going forward in the log.
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
//! Whenever that log is updated, the information about which closest past and future frames it
//! expects to be updated again is stored in [`RevMeta`]. If at these frames the log is not updated,
//! [`RevMeta::update`] will report that via the [`RevMetaUpdateErr::UpdateLogsMissed`] error that
//! contains a list of [`UpdateLogMissed`]. [`UpdateLogMissed::id`] contains the
//! [id of the `UpdateLog` instance] that was missed. This id is also logged at the INFO level when
//! it is set for an individual `UpdateLog`.
//!
//! When bevy's `track_location` cargo feature is active, [`UpdateLogMissed::last_update`] also
//! contains the location where the [`UpdateLog`] was updated the last time.
//!
//! Note however that when a [`RevQueue::ClearThenRunForward`]/[`RevQueue::ClearThenPause`] is
//! applied, all ids until then become invalid. This event is logged at the INFO level as well.
//! Every `UpdateLog` that updates after that will get new ids which is then logged again.
//!
//! Log clears happen not when the queue above is applied but at the first next update of each log
//! after that.
//!
//! ## Draining transitions
//!
//! Updating the transition logs with [`forward_push`] and [`forward_extend`] also returns draining
//! iterators.
//!
//! ### Example
//!
//! A [`UpdateLog::forward_past_len`] runs at [frame `42`](crate::meta::RevMeta::now) during
//! [`RevDirection::Forward`]. This is the first time this log updated. `UpdateLog` will then inform
//! [`RevMeta`] that there is no future frame it expects to run at during
//! [`RevDirection::ForwardLog`] but expects to run at frame `41` when going backward.
//!
//! When then [`UpdateLog::backward_log`] of this specific log runs at `41` during
//! [`RevDirection::BackwardLog`], this gets updated: Now there is no other frame in the past it
//! expects to run at, however it expects to run at frame `42` during [`RevDirection::ForwardLog`].
//!
//! If these updates during the [log directions] do not happen however, [`RevMeta`] will notice
//! that, which triggers the [`UpdateLogsMissed`] error right at the frame where the update was
//! missed. That way the `backward_log`/`forward_log` methods of transition logs that are updated
//! alongside will never fail with [`OutOfLog`].
//!
//! [`forward_push`]: TransitionLog::forward_push
//! [`forward_extend`]: TransitionsLog::forward_extend
//! [id of the `UpdateLog` instance]: UpdateLog::id
//! [`NonZeroU64::MAX`]: core::num::NonZeroU64::MAX
//! [`RevMeta`]: crate::meta::RevMeta
//! [`RevMeta::now`]: crate::meta::RevMeta::now
//! [`NotLog`]: crate::meta::NotLog
//! [`RevMeta::update`]: crate::meta::RevMeta::update
//! [global log range]: crate::meta::RevMeta::contains
//! [`RevDirection`]: crate::meta::RevDirection
//! [`RevDirection::Forward`]: crate::meta::RevDirection::Forward
//! [`Forward`]: crate::meta::RevDirection::Forward
//! [`RevDirection::BackwardLog`]: crate::meta::RevDirection::BackwardLog
//! [`BackwardLog`]: crate::meta::RevDirection::BackwardLog
//! [`RevDirection::ForwardLog`]: crate::meta::RevDirection::ForwardLog
//! [`ForwardLog`]: crate::meta::RevDirection::ForwardLog
//! [log directions]: crate::meta::RevDirection::is_log
//! [`RevQueue::ClearThenRunForward`]: crate::meta::RevQueue::ClearThenRunForward
//! [`RevQueue::ClearThenPause`]: crate::meta::RevQueue::ClearThenPause
//! [`RevMetaUpdateErr::UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [`UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [reversible scheduling]: crate::schedule::RevSchedule
//! [reversible commands]: crate::undo_redo::RevCommands
use core::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    iter::FusedIterator,
};
use std::collections::{VecDeque, vec_deque::Drain};

use bevy_ecs::change_detection::MaybeLocation;

pub(crate) use update::{
    PreUpdateKind,
    limit::{UpdateLogLimits, UpdateLogState},
};
pub use update::{UpdateLog, limit::UpdateLogId, limit::UpdateLogMissed};

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
/// still is an error, it will be because an update was missed and `UpdateLog` will cause that to be
/// reported at the end of the schedule.
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
        let mut this_range = gap_range.clone();

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
            let entry = unsafe { buffer.next().unwrap_unchecked() };
            deque.push_front(entry);
        }
        len => {
            deque.extend(buffer);
            deque.rotate_right(len);
        }
    }
}
