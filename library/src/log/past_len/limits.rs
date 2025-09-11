use std::{fmt::Debug, num::NonZeroU32, sync::atomic::AtomicU32};

use bevy::{ecs::change_detection::MaybeLocation, reflect::Reflect, utils::Parallel};

use crate::log::PreUpdateVariant;

/// Part of [`RevMeta`](crate::meta::RevMeta) that keeps track of [`PastLenLog`](super::PastLenLog)
/// updates and reports when such an update was expected for a present frame but did not happen.
#[derive(Reflect)]
pub(crate) struct PastLenLogLimits {
    /// Amount of [`PastLenLog`](super::PastLenLog) currently known to this struct
    past_len_ids: AtomicU32,

    #[reflect(ignore)]
    past_len_updates: Parallel<Vec<Vec<(u32, PastLenLimits)>>>,

    /// The most recent limits per [`PastLenLog`](super::PastLenLog), their [PastLenState::id] is
    /// represented by the index in this vector.
    past_len_limits: Vec<PastLenLimits>,
}

/// An bundling type for the state and the limits of an [`PastLenLog`](super::PastLenLog) update.
#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
struct PastLenUpdate {
    state: PastLenState,
    limits: PastLenLimits,
}

/// The state of a [`PastLenLog`](super::PastLenLog) that is needed to report [`PastLenLimit`]
/// and to react on log exits/clears.
#[derive(Reflect, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PastLenState {
    /// Unique id of a [`PastLenLog`](super::PastLenLog) to keep track how many logs are out there
    /// to reserve them space in [`PastLenLogLimits::past_len_limits`]. May change when
    /// [`RevMeta`](crate::meta::RevMeta) is [cleared](crate::meta::RevQueue::Clear).
    pub(super) id: u32,

    /// Counts how many times a log was updated in this frame.
    ///
    /// Note that, despire being a `NonZero` type, the minimum value indeed represents zero updates.
    /// What is important is that [`PastLenLimit`] updates can be ordered chronologically with this
    /// value. The NonZero type only offers a niche since this struct is stored in an `Option` in
    /// [`PastLenLog`](super::PastLenLog).
    updates_this_frame: NonZeroU32,

    /// Contains the most recent count of log exits that was witnessed by an
    /// [`PastLenLog`](super::PastLenLog).
    ///
    /// See [`RevMeta::log_exits`](crate::meta::RevMeta::log_exits).
    global_log_exits: u64,

    /// Contains the most recent count of log clears that was witnessed by an
    /// [`PastLenLog`](super::PastLenLog).
    ///
    /// See [`RevMeta::log_clears`](crate::meta::RevMeta::log_clears).
    global_log_clears: u64,
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
    /// `future`, unless during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    pub(crate) fn new_log(past: u64, future: u64, caller: MaybeLocation) -> Self {
        Self {
            past,
            future,
            last_update: caller,
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

impl Debug for PastLenLogLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PastLenLimits")
            .field("past_len_ids", &self.past_len_ids)
            .field("past_len_limits", &self.past_len_limits)
            .finish_non_exhaustive()
    }
}

struct UpdatesIter<'a> {
    past_len_updates: &'a mut Parallel<Vec<Vec<(u32, PastLenLimits)>>>,
    updates_this_frame: usize,
}

impl<'a> UpdatesIter<'a> {
    fn pop(&mut self) -> Option<(u32, PastLenLimits)> {
        self.past_len_updates
            .iter_mut()
            .flat_map(|local| local.get_mut(self.updates_this_frame))
            .flat_map(|updates| updates.pop())
            .next()
    }
}

impl<'a> Iterator for UpdatesIter<'a> {
    type Item = (u32, PastLenLimits);
    fn next(&mut self) -> Option<Self::Item> {
        match self.pop() {
            Some(id_and_limit) => Some(id_and_limit),
            None => {
                // try to get an update of the next higher updates_this_frame, if this still yields
                // None then all updates are drained as there would be no gap of updates, a log that
                // updated for the nth time surely updated for the (n-1)th time before if n is > 0
                self.updates_this_frame += 1;
                self.pop()

                // todo: make sure each frame the count of a log is reset as otherwise the above is
                // not correct
            }
        }
    }
}

impl PastLenLogLimits {
    pub(crate) fn new() -> Self {
        Self {
            past_len_ids: AtomicU32::new(0),
            past_len_updates: Parallel::default(),
            past_len_limits: Vec::new(),
        }
    }
    pub(crate) fn update_past_len_state(
        &self,
        state: &mut Option<PastLenState>,
        updated_this_frame_again: bool,
        global_log_exits: u64,
        global_log_clears: u64,
    ) -> PreUpdateVariant {
        let new_state = || {
            let id = self
                .past_len_ids
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            PastLenState {
                id,
                updates_this_frame: NonZeroU32::MIN,
                global_log_exits,
                global_log_clears,
            }
        };
        match state {
            Some(state) => {
                if state.global_log_clears < global_log_clears {
                    *state = new_state();
                    PreUpdateVariant::RemoveLog
                } else if state.global_log_exits < global_log_exits {
                    state.updates_this_frame = NonZeroU32::MIN;
                    state.global_log_exits = global_log_exits;
                    PreUpdateVariant::RemoveFuture
                } else if updated_this_frame_again { // todo: this might need extra case
                    state.updates_this_frame = state.updates_this_frame.checked_add(1).unwrap();
                    PreUpdateVariant::Nothing
                } else {
                    state.updates_this_frame = NonZeroU32::MIN;
                    PreUpdateVariant::Nothing
                }
            }
            None => {
                *state = Some(new_state());
                PreUpdateVariant::Nothing
            }
        }
    }
    pub(crate) fn clear(&mut self) {
        self.past_len_ids = AtomicU32::new(0);
        self.past_len_updates.iter_mut().for_each(Vec::clear);
        self.past_len_limits.clear();
    }
    pub(crate) fn check_past_len_limits(
        &mut self,
        now: u64,
        log: bool,
    ) -> Result<(), Vec<PastLenLogMissed>> {
        // size up self.past_len_limits if new PastLenLogs updated in the closure
        self.past_len_limits.resize(
            *self.past_len_ids.get_mut() as usize,
            PastLenLimits {
                // in case a PastLenLog inits its state without mutating afterwards, make these
                // bounds infallible
                past: u64::MIN,
                future: u64::MAX,

                // if an error points to this, something went wrong internally
                last_update: MaybeLocation::caller(),
            },
        );

        // update limits of PastLenLog instances that updated in the closure
        let iter = UpdatesIter {
            past_len_updates: &mut self.past_len_updates,
            updates_this_frame: 0
        };
        for (internal_id, limits) in iter {
            // if a PastLenLog pushed more than one limit, the most recent determines the limits,
            // so if one of the updates in a log frame was missed, this will cause an error
            self.past_len_limits[internal_id as usize] = limits;
        }

        if log {
            // check limits of all PastLenLog instances
            let mut past_len_logs_missed = Vec::new();
            for (index, limits) in self.past_len_limits.iter().enumerate() {
                let internal_id = index as u32;
                if now < limits.past || now > limits.future {
                    past_len_logs_missed.push(PastLenLogMissed {
                        id: internal_id,
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
            // unset future limits because logs just were or will be truncated
            self.past_len_limits
                .iter_mut()
                .for_each(|limit| limit.future = u64::MAX);
            Ok(())
        }
    }
    pub(super) fn push_past_len_update(&self, state: PastLenState, limits: PastLenLimits) {
        let v = &mut *self.past_len_updates.borrow_local_mut();
        let updates_this_frame = state.updates_this_frame.get() as usize - 1;
        let new_len = v.len().max(updates_this_frame);
        v.resize(new_len, Vec::new());
        v[updates_this_frame].push((state.id, limits));
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn updates_state() {
        let mut limits = PastLenLogLimits::new();
        let mut state = None;

        // initial set gives Nothing variant
        let variant = limits.update_past_len_state(&mut state, false, 0, 0);
        assert_eq!(variant, PreUpdateVariant::Nothing);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 0,
                updates_this_frame: NonZeroU32::MIN,
                global_log_exits: 0,
                global_log_clears: 0,
            })
        );

        // update at the same frame increases updates_this_frame
        let variant = limits.update_past_len_state(&mut state, true, 0, 0);
        assert_eq!(variant, PreUpdateVariant::Nothing);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 0,
                updates_this_frame: NonZeroU32::new(2).unwrap(),
                global_log_exits: 0,
                global_log_clears: 0,
            })
        );

        // update at another frame
        let variant = limits.update_past_len_state(&mut state, false, 0, 0);
        assert_eq!(variant, PreUpdateVariant::Nothing);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 0,
                updates_this_frame: NonZeroU32::MIN,
                global_log_exits: 0,
                global_log_clears: 0,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        let variant = limits.update_past_len_state(&mut state, true, 0, 0);
        assert_eq!(variant, PreUpdateVariant::Nothing);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 0,
                updates_this_frame: NonZeroU32::new(2).unwrap(),
                global_log_exits: 0,
                global_log_clears: 0,
            })
        );

        // increased log_exits gives DropFuture variant
        let variant = limits.update_past_len_state(&mut state, false, 1, 0);
        assert_eq!(variant, PreUpdateVariant::RemoveFuture);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 0,
                updates_this_frame: NonZeroU32::MIN,
                global_log_exits: 1,
                global_log_clears: 0,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        let variant = limits.update_past_len_state(&mut state, true, 1, 0);
        assert_eq!(variant, PreUpdateVariant::Nothing);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 0,
                updates_this_frame: NonZeroU32::new(2).unwrap(),
                global_log_exits: 1,
                global_log_clears: 0,
            })
        );

        // increased log_clears gived DropLog variant and resets everything
        limits.clear();
        let variant = limits.update_past_len_state(&mut state, false, 1, 1);
        assert_eq!(variant, PreUpdateVariant::RemoveLog);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 0,
                updates_this_frame: NonZeroU32::MIN,
                global_log_exits: 1,
                global_log_clears: 1,
            })
        );

        // the same is true if log_exits increased as well in the meantime
        // do not clear id counter to demonstrate this issues a new, potentially different id
        // limits.clear();
        let variant = limits.update_past_len_state(&mut state, false, 2, 2);
        assert_eq!(variant, PreUpdateVariant::RemoveLog);
        assert_eq!(
            state,
            Some(PastLenState {
                id: 1,
                updates_this_frame: NonZeroU32::MIN,
                global_log_exits: 2,
                global_log_clears: 2,
            })
        );
    }

    #[test]
    fn push_and_assert_limits() {
        let mut limits = PastLenLogLimits::new();

        // add a past limit of 1
        let mut past_state = None;
        let past_limit = PastLenLimits::new_log(1, u64::MAX, MaybeLocation::caller());
        limits.update_past_len_state(&mut past_state, false, 0, 0);
        limits.push_past_len_update(past_state.unwrap(), past_limit);

        // add a future limit of 1
        let mut future_state = None;
        let future_limit = PastLenLimits::new_log(u64::MIN, 1, MaybeLocation::caller());
        limits.update_past_len_state(&mut future_state, false, 0, 0);
        limits.push_past_len_update(future_state.unwrap(), future_limit);

        // 1 is in both limits
        let result = limits.check_past_len_limits(1, true);
        assert_eq!(result, Ok(()));

        // 0 is breaching the past limit
        let result = limits.check_past_len_limits(0, true);
        let past_missed = Err(vec![PastLenLogMissed {
            id: past_state.unwrap().id,
            last_update: past_limit.last_update,
        }]);
        assert_eq!(result, past_missed);

        // 2 is breaching the future limit
        let result = limits.check_past_len_limits(2, true);
        let future_missed = Err(vec![PastLenLogMissed {
            id: future_state.unwrap().id,
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
