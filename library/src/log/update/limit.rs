//! This module contains [`UpdateLogLimits`] which is part of [`RevMeta`](crate::meta::RevMeta).
//!
//! [`UpdateLog`](super::UpdateLog) sends [`UpdateLogLimit`] to that struct so `RevMeta` can report
//! errors if it did not update at the current frame as it did during
//! [`RevDirection::NotLog`](crate::meta::RevDirection::NotLog).

use alloc::{boxed::Box, vec::Vec};
use bevy_ecs::change_detection::MaybeLocation;
use bevy_utils::Parallel;
use core::{
    fmt::{Debug, Formatter, Result as FmtResult},
    panic::Location,
    sync::atomic::AtomicU32,
};
use nonmax::NonMaxU32;

use crate::log::update::UpdateLocation;

/// Part of [`RevMeta`](crate::meta::RevMeta) that keeps track of [`UpdateLog`](super::UpdateLog)
/// updates and reports when such an update was expected for a present frame but did not happen.
#[derive(Default)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub(crate) struct UpdateLogLimits {
    /// Amount of [`UpdateLog`](super::UpdateLog) currently known to this struct
    past_len_count: AtomicU32,

    /// Amount of times [`update`](Self::update) was called.
    limits_updates: u64,

    /// The locals contain 2D vectors:
    ///
    /// - the outer vector's index represents with which [`UpdateLogState::updates_this_frame`]
    ///   value limits were pushed.
    /// - the inner vector contains all limits to said `updates_this_frame`.
    /// - the [`NonMaxU32`] value contains the id of the limit this log is associated with, the
    ///   draining order will ensure only the last limit of a specific log is stored in
    ///   [`Self::update_log_limits`].
    #[cfg_attr(feature = "reflect", reflect(ignore))]
    #[allow(clippy::type_complexity)]
    update_log_updates: Box<Parallel<Vec<Vec<(NonMaxU32, UpdateLogLimit)>>>>,

    /// The most recent limits per [`UpdateLog`](super::UpdateLog) with [UpdateLogState::index]
    /// being used as the index in this vector.
    update_log_limits: Vec<UpdateLogLimit>,
}

impl Debug for UpdateLogLimits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UpdateLogLimits")
            .field("past_len_count", &self.past_len_count)
            .field("limits_updates", &self.limits_updates)
            .field("update_log_limits", &self.update_log_limits)
            .field(
                "update_log_updates (this thread)",
                &*self.update_log_updates.borrow_local_mut(),
            )
            .finish_non_exhaustive() // update_log_updates may contain updates from another thread
    }
}

impl UpdateLogLimits {
    /// Update the given [`UpdateLogState`] or init it if its `None`.
    ///
    /// Return which operation the owning [`UpdateLog`](super::UpdateLog) has to do before any
    /// other mutation.
    #[track_caller]
    pub(crate) fn set_update_state(
        &self,
        state: &mut Option<UpdateLogState>,
        caller: MaybeLocation<Option<&'static Location>>,
    ) {
        match state {
            None => {
                let index = self
                    .past_len_count
                    .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                // exhausted maximum number of `UpdateLog` if this panics
                let index = NonMaxU32::new(index).unwrap();
                *state = Some(UpdateLogState {
                    index,
                    updates_this_frame: 0,
                    witnessed_limits_updates: self.limits_updates,
                });
                if let Some(caller) = caller.into_option().flatten() {
                    bevy_log::info!(
                        "A `UpdateLog` with the index `{index}` was initiated at {caller},  this \
                        index remains valid until a `RevQueue::Clear` is applied, after that the \
                        index could be assigned to a different UpdateLog"
                    );
                }
            }
            Some(state) => {
                if state.witnessed_limits_updates != self.limits_updates {
                    // this log did not update at this frame yet
                    state.witnessed_limits_updates = self.limits_updates;
                    state.updates_this_frame = 0;
                }
            }
        }
    }

    /// Forget all limits and any [`UpdateLog`](super::UpdateLog) that did register so far.
    ///
    /// `UpdateLog`s have to reregister themselves with a new [`UpdateLogState`].
    pub(crate) fn clear(&mut self) {
        self.past_len_count = 0.into();
        self.limits_updates = 0;
        self.update_log_updates
            .iter_mut()
            .flatten()
            .for_each(Vec::clear);
        self.update_log_limits.clear();
    }

    /// Update internal list of [`UpdateLogLimit`] with new updates and check if `now` is breaching
    /// any of the limits.
    pub(crate) fn update(&mut self, now: u64, log: bool) -> Result<(), Vec<UpdateLogMissed>> {
        // size up update_log_limits if new `UpdateLog`s updated since the last call of this
        self.update_log_limits.resize(
            *self.past_len_count.get_mut() as usize,
            UpdateLogLimit {
                // in case a new `UpdateLog` called `set_update_state` but not
                // `push_limit`, these bounds need to be infallible
                past: u64::MIN,
                future: u64::MAX,

                // if an error points to this, something went wrong internally
                last_update: MaybeLocation::new(None),
            },
        );

        // update limits of `UpdateLog` instances that updated since the last call of this
        let iter = UpdatesIter {
            update_log_updates: &mut self.update_log_updates,
            skip_locals: 0,
            updates_this_frame: 0,
        };
        for (index, limits) in iter {
            // if a `UpdateLog` pushed more than one limit, the most recent determines the limits,
            // so if one of the updates in a log frame was missed, this will error later below
            self.update_log_limits[index] = limits;
        }

        // exhausted maximum number of `UpdateLog` updates if this panics
        self.limits_updates = self.limits_updates.checked_add(1).unwrap();

        // there should be no gaps in the 2D vector, otherwise that would be a bug here
        assert_eq!(
            self.update_log_updates
                .iter_mut()
                .flatten()
                .map(|v| v.len())
                .sum::<usize>(),
            0
        );

        if log {
            // check limits of all known `UpdateLog` instances
            let mut update_logs_missed = Vec::new();
            for (index, limits) in self.update_log_limits.iter().enumerate() {
                if now < limits.past || now > limits.future {
                    update_logs_missed.push(UpdateLogMissed {
                        index,
                        last_update: limits.last_update,
                    });
                }
            }
            if update_logs_missed.is_empty() {
                Ok(())
            } else {
                Err(update_logs_missed)
            }
        } else {
            // unset future limits because logs just were or will be truncated from their future
            self.update_log_limits
                .iter_mut()
                .for_each(|limit| limit.future = u64::MAX);
            Ok(())
        }
    }

    /// Update the logged [`UpdateLogLimit`] associated to this `state` with the given `limit`.
    pub(super) fn push_limit(&self, state: &mut UpdateLogState, limit: UpdateLogLimit) {
        debug_assert_eq!(state.witnessed_limits_updates, self.limits_updates);

        // push update, extend outer vector if needed
        let v = &mut *self.update_log_updates.borrow_local_mut();
        let index = state.updates_this_frame as usize;
        let new_len = v.len().max(index + 1);
        v.resize(new_len, Vec::new());
        v[index].push((state.index, limit));

        // the next update at this frame should come with a higher value
        state.updates_this_frame += 1;
    }
}

/// The state of a [`UpdateLog`](super::UpdateLog) that is needed to report [`UpdateLogLimit`]
/// and to react on log exits/clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UpdateLogState {
    /// Unique index of a [`UpdateLog`](super::UpdateLog) to keep track how many logs are out there
    /// to reserve them space in [`UpdateLogLimits::update_log_limits`]. May change when
    /// [`RevMeta`](crate::meta::RevMeta) is [cleared](crate::meta::RevQueue::ClearThenRunForward).
    ///
    /// Is `NonMax` to offer a niche since `Self` is stored in an `Option` at `UpdateLog`.
    pub(super) index: NonMaxU32,

    /// Counts how many times this [`UpdateLog`](super::UpdateLog) was
    /// [updated](UpdateLogLimits::set_update_state) in this frame.
    updates_this_frame: u32,

    /// Contains the most recent [`UpdateLogLimits::limits_updates`] value that was witnessed by this
    /// [`UpdateLog`](super::UpdateLog).
    witnessed_limits_updates: u64,
}

/// A limit for [`RevMeta::now`](crate::meta::RevMeta::now) that, if breached, indicates that a
/// [`UpdateLog`](super::UpdateLog) unexpectedly did not update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub(crate) struct UpdateLogLimit {
    /// The minimum value for [`RevMeta::now`](crate::meta::RevMeta::now). At the next lower value
    /// the [`UpdateLog`](super::UpdateLog) should update and send a new, lower limit to
    /// [`UpdateLogLimits`].
    ///
    /// May be larger than [`Self::future`] when a `UpdateLog` did update this frame repeatedly
    /// but is still expecting to update again before the frame is over.
    ///
    /// Is some frame at or below [`RevMeta::past_end`](crate::meta::RevMeta::past_end) if
    /// `UpdateLog` has no further updates in its past.
    past: u64,

    /// The maximum value for [`RevMeta::now`](crate::meta::RevMeta::now). At the next higher value
    /// the [`UpdateLog`](super::UpdateLog) should update and send a new, higher limit to
    /// [`UpdateLogLimits`].
    ///
    /// May be smaller than [`Self::past`] when a `UpdateLog` did update this frame repeatedly
    /// but is still expecting to update again before the frame is over.
    ///
    /// Is [`u64::MAX`] if `UpdateLog` has no further updates in its future.
    ///
    /// Will be set to `u64::MAX` at [`RevDirection::NotLog`](crate::meta::RevDirection::NotLog).
    future: u64,

    /// The last location where [`UpdateLog`](super::UpdateLog) was updated. Is empty if bevy's
    /// `track_location` cargo feature is not used.
    last_update: UpdateLocation,
}

impl UpdateLogLimit {
    /// A new limit during an [`RevDirection::NotLog`](crate::meta::RevDirection::NotLog) update.
    ///
    /// Restricts [`RevMeta::now`](crate::meta::RevMeta::now) to not go below `now`.
    pub(crate) fn new_forward(now: u64, caller: UpdateLocation) -> Self {
        Self {
            past: now,
            future: u64::MAX,
            last_update: caller,
        }
    }

    /// A new limit during an [`RevDirection::ForwardLog`](crate::meta::RevDirection::ForwardLog)/
    /// [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) update.
    ///
    /// Restricts [`RevMeta::now`](crate::meta::RevMeta::now) to not go below `past` or above
    /// `future`, except during [`RevDirection::NotLog`](crate::meta::RevDirection::NotLog).
    pub(crate) fn new_log(past: u64, future: u64, caller: UpdateLocation) -> Self {
        Self {
            past,
            future,
            last_update: caller,
        }
    }
}

/// Iterator that pops all newly queued [`UpdateLogLimit`].
///
/// Makes sure the limits are yielded in chronological order for each log.
struct UpdatesIter<'a> {
    /// The queued limits
    update_log_updates: &'a mut Parallel<Vec<Vec<(NonMaxU32, UpdateLogLimit)>>>,

    /// The [`Parallel`] locals that can be skipped because they contain no more [`UpdateLogLimit`]
    /// for the current [`Self::updates_this_frame`].
    skip_locals: usize,

    /// The [`UpdateLogState::updates_this_frame`] value that was present for the [`UpdateLogLimit`]
    /// which are currently returned by the iterator.
    updates_this_frame: usize,
}

impl<'a> UpdatesIter<'a> {
    /// Pop the next item for the current [`Self::updates_this_frame`].
    fn pop(&mut self) -> Option<(usize, UpdateLogLimit)> {
        self.update_log_updates
            .iter_mut()
            .skip(self.skip_locals)
            .flat_map(|local| {
                let update = local.get_mut(self.updates_this_frame).and_then(Vec::pop);
                if update.is_none() {
                    // if this local exhausted limits for this updates_this_frame, skip it the next
                    // time this method is called
                    self.skip_locals += 1;
                }
                update
            })
            .next()
            .map(|(index, limits)| (index.get() as usize, limits))
    }
}

impl<'a> Iterator for UpdatesIter<'a> {
    type Item = (usize, UpdateLogLimit);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.pop().or_else(|| {
            // Try to get an update of the next higher updates_this_frame. If this still yields None
            // then all updates are drained as there would be no gap of updates. A log that updated
            // for the nth time surely updated for the (n-1)th time before if n is > 0.
            self.skip_locals = 0;
            self.updates_this_frame += 1;
            self.pop()
        })
    }
}

/// Identifies a [`UpdateLog`](super::UpdateLog) that should have been updated at the current
/// [frame](crate::meta::RevMeta::now) but did not.
///
/// This error can be manually received from [`RevMeta::update`](crate::meta::RevMeta::update). If
/// [`run_rev_update`](crate::schedule::run_rev_update) is used, this error, if it
/// occurs, is given to the default error handler.
#[derive(PartialEq, Clone, Copy)]
pub struct UpdateLogMissed {
    /// The internal index of the [`UpdateLog`](super::UpdateLog). This is logged at the INFO level
    /// whenever it is set.
    pub index: usize,

    /// The last location in the code where the [`UpdateLog`](super::UpdateLog) was updated.
    ///
    /// If this is `None`, the error is cause by a `bevy_oozlum` internal log. In that case please
    /// report a bug with a minimal reproducing example.
    ///
    /// Requires to use bevy's `track_location` feature.
    pub last_update: UpdateLocation,
}

impl Debug for UpdateLogMissed {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self.last_update.into_option() {
            None => write!(f, "{}", self.index),
            Some(None) => write!(f, "{} (bevy_oozlum bug)", self.index),
            Some(Some(location)) => write!(f, "{} (modified {location})", self.index),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use alloc::vec;

    #[test]
    fn updates_state() {
        let mut limits = UpdateLogLimits::default();
        let mut state = None;
        let caller = MaybeLocation::new(None);
        let no_caller = MaybeLocation::new(None);

        // initial set gives Nothing variant
        limits.set_update_state(&mut state, no_caller);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                witnessed_limits_updates: 0,
            })
        );

        // pushing update increases updates_this_frame
        limits.push_limit(
            state.as_mut().unwrap(),
            UpdateLogLimit::new_forward(1, caller),
        );
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 1,
                witnessed_limits_updates: 0,
            })
        );
        limits.update(2, false).unwrap();

        // update at another frame
        limits.set_update_state(&mut state, no_caller);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                witnessed_limits_updates: 1,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        limits.push_limit(
            state.as_mut().unwrap(),
            UpdateLogLimit::new_forward(2, caller),
        );
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 1,
                witnessed_limits_updates: 1,
            })
        );
        limits.update(3, false).unwrap();

        // increased log_exits gives DropFuture variant
        limits.set_update_state(&mut state, no_caller);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                witnessed_limits_updates: 2,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        limits.push_limit(
            state.as_mut().unwrap(),
            UpdateLogLimit::new_forward(3, caller),
        );
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 1,
                witnessed_limits_updates: 2,
            })
        );
        limits.update(4, false).unwrap();

        // when clearing limits, UpdateLog also unsets state, generate new state
        limits.clear();
        state = None;
        limits.set_update_state(&mut state, no_caller);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                witnessed_limits_updates: 0,
            })
        );

        // another UpdateLog receives a different index
        state = None;
        limits.set_update_state(&mut state, no_caller);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ONE,
                updates_this_frame: 0,
                witnessed_limits_updates: 0,
            })
        );
    }

    #[test]
    fn push_and_assert_limits() {
        let mut limits = UpdateLogLimits::default();
        let no_caller = MaybeLocation::new(None);

        // add a past limit of 1
        let mut past_state = None;
        let past_limit = UpdateLogLimit::new_log(1, u64::MAX, MaybeLocation::caller().map(Some));
        limits.set_update_state(&mut past_state, no_caller);
        limits.push_limit(past_state.as_mut().unwrap(), past_limit);

        // add a future limit of 1
        let mut future_state = None;
        let future_limit = UpdateLogLimit::new_log(u64::MIN, 1, MaybeLocation::caller().map(Some));
        limits.set_update_state(&mut future_state, no_caller);
        limits.push_limit(future_state.as_mut().unwrap(), future_limit);

        // 1 is in both limits
        let result = limits.update(1, true);
        assert_eq!(result, Ok(()));

        // 0 is breaching the past limit
        let result = limits.update(0, true);
        let past_missed = Err(vec![UpdateLogMissed {
            index: past_state.unwrap().index.get() as usize,
            last_update: past_limit.last_update,
        }]);
        assert_eq!(result, past_missed);

        // 2 is breaching the future limit
        let result = limits.update(2, true);
        let future_missed = Err(vec![UpdateLogMissed {
            index: future_state.unwrap().index.get() as usize,
            last_update: future_limit.last_update,
        }]);
        assert_eq!(result, future_missed);

        // 1 is in both limits, false unsets future limits
        let result = limits.update(1, false);
        assert_eq!(result, Ok(()));

        // 0 is breaching the past limit
        let result = limits.update(0, true);
        assert_eq!(result, past_missed);

        // 2 is no longer breaching the future limit
        let result = limits.update(2, true);
        assert_eq!(result, Ok(()));
    }
}
