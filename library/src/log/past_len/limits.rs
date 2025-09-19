//! This module contains [`PastLenLogLimits`] which is part of [`RevMeta`](crate::meta::RevMeta).
//!
//! [`PastLenLog`](super::PastLenLog) interacts with that struct in two ways:
//!
//! 1. Updating [`PastLenState`] in the log to determine which [`PreUpdateVariant`] is returned in
//!    [`pre_update`](super::PastLenLog::pre_update).
//! 2. Sending [`PastLenLimits`] at
//!    [`update_and_get_past_len`](super::PastLenLog::update_and_get_past_len),
//!    [`forward_log`](super::PastLenLog::forward_log) and
//!    [`backward_log`](super::PastLenLog::backward_log). When `RevMeta` is updating
//!   `PastLenLogLimits`, an error can be generated if a `PastLenLog` did not update (often enough)
//!   at the current frame as it did during
//!   [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).

use super::PreUpdateVariant;
use bevy::{ecs::change_detection::MaybeLocation, reflect::Reflect, utils::Parallel};
use nonmax::NonMaxU32;
use std::{fmt::Debug, sync::atomic::AtomicU32};

/// Part of [`RevMeta`](crate::meta::RevMeta) that keeps track of [`PastLenLog`](super::PastLenLog)
/// updates and reports when such an update was expected for a present frame but did not happen.
#[derive(Reflect, Default)]
pub(crate) struct PastLenLogLimits {
    /// Amount of [`PastLenLog`](super::PastLenLog) currently known to this struct
    past_len_ids: AtomicU32,

    /// Amount of times [`check_past_len_limits`](Self::check_past_len_limits) was called.
    updates: u64,

    /// The locals contain 2D vectors:
    ///
    /// - the outer vector's index represents with which [`PastLenState::updates_this_frame`] value
    /// limits were pushed
    /// - the inner vector contains all limits to said `updates_this_frame`
    #[reflect(ignore)]
    past_len_log_updates: Parallel<Vec<Vec<(NonMaxU32, PastLenLimits)>>>,

    /// The most recent limits per [`PastLenLog`](super::PastLenLog), their [PastLenState::id] is
    /// represented by the index in this vector.
    past_len_log_limits: Vec<PastLenLimits>,
}

impl Debug for PastLenLogLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PastLenLogLimits")
            .field("past_len_ids", &self.past_len_ids)
            .field("updates", &self.updates)
            .field("past_len_log_limits", &self.past_len_log_limits)
            .field(
                "past_len_log_updates (this thread)",
                &*self.past_len_log_updates.borrow_local_mut(),
            )
            .finish_non_exhaustive() // past_len_log_updates may contain updates for another thread
    }
}

impl PastLenLogLimits {
    /// Update the given [`PastLenState`] or init it if its `None`.
    ///
    /// Return which operation the owning [`PastLenlog`](super::PastLenLog) has to do before any
    /// other mutation.
    pub(crate) fn update_past_len_state(
        &self,
        state: &mut Option<PastLenState>,
        meta_log_exits: u64,
        meta_log_clears: u64,
    ) -> PreUpdateVariant {
        let new_state = || {
            let id = self
                .past_len_ids
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let id = NonMaxU32::new(id).expect("exhausted maximum number of PastLenLog");
            PastLenState {
                id,
                updates_this_frame: 0,
                limits_updates: self.updates,
                meta_log_exits,
                meta_log_clears,
            }
        };
        match state {
            Some(state) => {
                if state.meta_log_clears < meta_log_clears {
                    // meta was cleared in the meantime
                    *state = new_state();
                    PreUpdateVariant::RemoveLog
                } else if state.meta_log_exits < meta_log_exits {
                    // meta ran at not-log in the meantime
                    state.limits_updates = self.updates;
                    state.meta_log_exits = meta_log_exits;
                    state.updates_this_frame = 0;
                    PreUpdateVariant::RemoveFuture
                } else {
                    if state.limits_updates != self.updates {
                        // this log did not update at this frame yet
                        state.limits_updates = self.updates;
                        state.updates_this_frame = 0;
                    }
                    PreUpdateVariant::Nothing
                }
            }
            None => {
                // init state, no further operation as it starts empty
                *state = Some(new_state());
                PreUpdateVariant::Nothing
            }
        }
    }

    /// Forget all limits and any [`PastLenlog`](super::PastLenLog) that did register so far.
    ///
    /// `PastLenLog`s have to reregister themselves with a new [`PastLenState`].
    pub(crate) fn clear(&mut self) {
        self.past_len_ids = AtomicU32::new(0);
        self.updates = 0;
        self.past_len_log_updates
            .iter_mut()
            .flatten()
            .for_each(Vec::clear);
        self.past_len_log_limits.clear();
    }

    /// Update internal list of [`PastLenLimits`] with new updates and check if `now` is breaching
    /// any of the limits.
    pub(crate) fn check_past_len_limits(
        &mut self,
        now: u64,
        log: bool,
    ) -> Result<(), Vec<PastLenLogMissed>> {
        // size up past_len_limits if new PastLenLogs updated since the last call of this
        self.past_len_log_limits.resize(
            *self.past_len_ids.get_mut() as usize,
            PastLenLimits {
                // in case a new PastLenLog called update_past_len_state but not
                // push_past_len_update, these bounds need to be infallible
                past: u64::MIN,
                future: u64::MAX,

                // if an error points to this, something went wrong internally
                last_update: MaybeLocation::caller(),
            },
        );

        // update limits of PastLenLog instances that updated since the last call of this
        let iter = UpdatesIter {
            past_len_updates: &mut self.past_len_log_updates,
            skip_locals: 0,
            updates_this_frame: 0,
        };
        for (index, limits) in iter {
            // if a PastLenLog pushed more than one limit, the most recent determines the limits,
            // so if one of the updates in a log frame was missed, this will error later below
            self.past_len_log_limits[index] = limits;
        }

        self.updates = self
            .updates
            .checked_add(1)
            .expect("exhausted maximum number of updates");

        assert_eq!(
            self.past_len_log_updates
                .iter_mut()
                .flatten()
                .map(|v| v.len())
                .sum::<usize>(),
            0
        );

        if log {
            // check limits of all known PastLenLog instances
            let mut past_len_logs_missed = Vec::new();
            for (index, limits) in self.past_len_log_limits.iter().enumerate() {
                if now < limits.past || now > limits.future {
                    past_len_logs_missed.push(PastLenLogMissed {
                        id: index as u32,
                        last_update: limits.last_update,
                    });
                }
            }
            if past_len_logs_missed.is_empty() {
                Ok(())
            } else {
                Err(past_len_logs_missed)
            }
        } else {
            // unset future limits because logs just were or will be truncated from their future
            self.past_len_log_limits
                .iter_mut()
                .for_each(|limit| limit.future = u64::MAX);
            Ok(())
        }
    }

    /// Update the logged [`PastLenLimits`] associated to this `state` with the given `limits`.
    ///
    /// # Panic
    ///
    /// This method panics if `state` is was not updated this frame via
    /// [`PastLenLog::pre_update`](super::PastLenLog::pre_update).
    pub(super) fn push_past_len_update(
        &self,
        state: &mut Option<PastLenState>,
        limits: PastLenLimits,
    ) {
        // get and verify state
        const ERR: &str = "PastLenLog did not run `pre_update` this reversible frame";
        let state = state.as_mut().expect(ERR);
        assert_eq!(state.limits_updates, self.updates, "{ERR}");

        // push update, extend outer vector if needed
        let v = &mut *self.past_len_log_updates.borrow_local_mut();
        let index = state.updates_this_frame as usize;
        let new_len = v.len().max(index + 1);
        v.resize(new_len, Vec::new());
        v[index].push((state.id, limits));

        // the next update at this frame should come with a higher value
        state.updates_this_frame += 1;
    }
}

/// The state of a [`PastLenLog`](super::PastLenLog) that is needed to report [`PastLenLimit`]
/// and to react on log exits/clears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PastLenState {
    /// Unique id of a [`PastLenLog`](super::PastLenLog) to keep track how many logs are out there
    /// to reserve them space in [`PastLenLogLimits::past_len_limits`]. May change when
    /// [`RevMeta`](crate::meta::RevMeta) is [cleared](crate::meta::RevQueue::Clear).
    ///
    /// Is `NonMax` to offer a niche since `Self` is stored in an `Option` at `PastLenLog`.
    pub(super) id: NonMaxU32,

    /// Counts how many times this [`PastLenLog`](super::PastLenLog) was updated in this frame.
    updates_this_frame: u32,

    limits_updates: u64,

    /// Contains the most recent global count of log exits that was witnessed by this
    /// [`PastLenLog`](super::PastLenLog).
    ///
    /// See [`RevMeta::log_exits`](crate::meta::RevMeta::log_exits).
    meta_log_exits: u64,

    /// Contains the most recent global count of log clears that was witnessed by this
    /// [`PastLenLog`](super::PastLenLog).
    ///
    /// See [`RevMeta::log_clears`](crate::meta::RevMeta::log_clears).
    meta_log_clears: u64,
}

/// A limit for [`RevMeta::now`](crate::meta::RevMeta::now) that, if breached, indicates that a
/// [`PastLenLog`](super::PastLenLog) unexpectedly did not update.
#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PastLenLimits {
    /// The minimum value for [`RevMeta::now`](crate::meta::RevMeta::now). At the next lower value
    /// the [`PastLenLog`](super::PastLenLog) should update and send a new, lower limit to
    /// [`PastLenLogLimits`].
    ///
    /// May be larger than [`Self::future`] when a `PastLenLog` did update this frame repeatedly
    /// but is still expecting to update again before the frame is over.
    ///
    /// Is some frame at or below [`RevMeta::past_end`](crate::meta::RevMeta::past_end) if
    /// `PastLenLog` has no further updates in its past.
    past: u64,

    /// The maximum value for [`RevMeta::now`](crate::meta::RevMeta::now). At the next higher value
    /// the [`PastLenLog`](super::PastLenLog) should update and send a new, higher limit to
    /// [`PastLenLogLimits`].
    ///
    /// May be smaller than [`Self::past`] when a `PastLenLog` did update this frame repeatedly
    /// but is still expecting to update again before the frame is over.
    ///
    /// Is [`u64::MAX`] if `PastLenLog` has no further updates in its future.
    ///
    /// Will be set to `u64::MAX` at [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    future: u64,

    /// The last location when [`PastLenLog`](super::PastLenLog) was updated. Is empty if the
    /// `track_location` feature is not used at Bevy.
    last_update: MaybeLocation,
}

impl PastLenLimits {
    /// A new limit during an [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG) update.
    ///
    /// Restricts [`RevMeta::now`](crate::meta::RevMeta::now) to not go below `now`.
    pub(crate) fn new_not_log(now: u64, caller: MaybeLocation) -> Self {
        Self {
            past: now,
            future: u64::MAX,
            last_update: caller,
        }
    }

    /// A new limit during an [`RevDirection::FORWARD_LOG`](crate::meta::RevDirection::FORWARD_LOG)/
    /// [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) update.
    ///
    /// Restricts [`RevMeta::now`](crate::meta::RevMeta::now) to not go below `past` or above
    /// `future`, except during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    pub(crate) fn new_log(past: u64, future: u64, caller: MaybeLocation) -> Self {
        Self {
            past,
            future,
            last_update: caller,
        }
    }
}

/// Iterator that pops all newly queued [`PastLenLimits`].
///
/// Makes sure the limits are yielded in chronological order for each log.
struct UpdatesIter<'a> {
    /// The queued limits
    past_len_updates: &'a mut Parallel<Vec<Vec<(NonMaxU32, PastLenLimits)>>>,

    /// The [`Parallel`] locals that can be skipped because they contain no more [`PastLenLimits`]
    /// for the current [`Self::updates_this_frame`].
    skip_locals: usize,

    /// The [`PastLenState::updates_this_frame`] value that was present for the [`PastLenLimits`]
    /// which are currently returned by the iterator.
    updates_this_frame: usize,
}

impl<'a> UpdatesIter<'a> {
    /// Pop the next item for the current [`Self::updates_this_frame`].
    fn pop(&mut self) -> Option<(usize, PastLenLimits)> {
        self.past_len_updates
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
            .map(|(id, limits)| (id.get() as usize, limits))
    }
}

impl<'a> Iterator for UpdatesIter<'a> {
    type Item = (usize, PastLenLimits);
    fn next(&mut self) -> Option<Self::Item> {
        match self.pop() {
            Some(index_and_limit) => Some(index_and_limit),
            None => {
                // Try to get an update of the next higher updates_this_frame. If this still yields
                // None then all updates are drained as there would be no gap of updates. A log that
                // updated for the nth time surely updated for the (n-1)th time before if n is > 0.
                // See also the comment in `RevMeta::update_past_len_state`.
                self.skip_locals = 0;
                self.updates_this_frame += 1;
                self.pop()
            }
        }
    }
}

/// Identifies a [`PastLenLog`](super::PastLenLog) that should have been updated at the current
/// [frame](crate::meta::RevMeta::now) but did not.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct PastLenLogMissed {
    /// The internal [id](super::PastLenLog::id) of the [`PastLenLog`](super::PastLenLog).
    pub id: u32,

    /// The last location in the code where the [`PastLenLog`](super::PastLenLog) was updated.
    ///
    /// Requires to use bevy's `track_location` feature.
    pub last_update: MaybeLocation,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn updates_state() {
        let mut limits = PastLenLogLimits::default();
        let mut state = None;
        let caller = MaybeLocation::caller();

        // initial set gives Nothing variant
        let variant = limits.update_past_len_state(&mut state, 0, 0);
        assert_eq!(variant, PreUpdateVariant::Nothing);
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 0,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );

        // pushing update increases updates_this_frame
        limits.push_past_len_update(&mut state, PastLenLimits::new_not_log(1, caller));
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ZERO,
                updates_this_frame: 1,
                limits_updates: 0,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );
        limits.check_past_len_limits(2, false).unwrap();

        // update at another frame
        let variant = limits.update_past_len_state(&mut state, 0, 0);
        assert_eq!(variant, PreUpdateVariant::Nothing);
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 1,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        limits.push_past_len_update(&mut state, PastLenLimits::new_not_log(2, caller));
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ZERO,
                updates_this_frame: 1,
                limits_updates: 1,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );
        limits.check_past_len_limits(3, false).unwrap();

        // increased log_exits gives DropFuture variant
        let variant = limits.update_past_len_state(&mut state, 1, 0);
        assert_eq!(variant, PreUpdateVariant::RemoveFuture);
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 2,
                meta_log_exits: 1,
                meta_log_clears: 0,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        limits.push_past_len_update(&mut state, PastLenLimits::new_not_log(3, caller));
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ZERO,
                updates_this_frame: 1,
                limits_updates: 2,
                meta_log_exits: 1,
                meta_log_clears: 0,
            })
        );
        limits.check_past_len_limits(4, false).unwrap();

        // increased log_clears gived DropLog variant and resets everything
        limits.clear();
        let variant = limits.update_past_len_state(&mut state, 1, 1);
        assert_eq!(variant, PreUpdateVariant::RemoveLog);
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 0,
                meta_log_exits: 1,
                meta_log_clears: 1,
            })
        );
        limits.check_past_len_limits(5, false).unwrap();

        // the same is true if log_exits increased as well in the meantime
        // do not clear id counter to demonstrate this issues a new, potentially different id
        // limits.clear();
        let variant = limits.update_past_len_state(&mut state, 2, 2);
        assert_eq!(variant, PreUpdateVariant::RemoveLog);
        assert_eq!(
            state,
            Some(PastLenState {
                id: NonMaxU32::ONE,
                updates_this_frame: 0,
                limits_updates: 1,
                meta_log_exits: 2,
                meta_log_clears: 2,
            })
        );
    }

    #[test]
    fn push_and_assert_limits() {
        let mut limits = PastLenLogLimits::default();

        // add a past limit of 1
        let mut past_state = None;
        let past_limit = PastLenLimits::new_log(1, u64::MAX, MaybeLocation::caller());
        limits.update_past_len_state(&mut past_state, 0, 0);
        limits.push_past_len_update(&mut past_state, past_limit);

        // add a future limit of 1
        let mut future_state = None;
        let future_limit = PastLenLimits::new_log(u64::MIN, 1, MaybeLocation::caller());
        limits.update_past_len_state(&mut future_state, 0, 0);
        limits.push_past_len_update(&mut future_state, future_limit);

        // 1 is in both limits
        let result = limits.check_past_len_limits(1, true);
        assert_eq!(result, Ok(()));

        // 0 is breaching the past limit
        let result = limits.check_past_len_limits(0, true);
        let past_missed = Err(vec![PastLenLogMissed {
            id: past_state.unwrap().id.get(),
            last_update: past_limit.last_update,
        }]);
        assert_eq!(result, past_missed);

        // 2 is breaching the future limit
        let result = limits.check_past_len_limits(2, true);
        let future_missed = Err(vec![PastLenLogMissed {
            id: future_state.unwrap().id.get(),
            last_update: future_limit.last_update,
        }]);
        assert_eq!(result, future_missed);

        // 1 is in both limits, false unsets future limits
        let result = limits.check_past_len_limits(1, false);
        assert_eq!(result, Ok(()));

        // 0 is breaching the past limit
        let result = limits.check_past_len_limits(0, true);
        assert_eq!(result, past_missed);

        // 2 is no longer breaching the future limit
        let result = limits.check_past_len_limits(2, true);
        assert_eq!(result, Ok(()));
    }
}
