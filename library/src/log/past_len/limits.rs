use std::{fmt::Debug, num::NonZeroU32, sync::atomic::AtomicU32};

use bevy::{ecs::change_detection::MaybeLocation, reflect::Reflect, utils::Parallel};

use crate::log::PreUpdateVariant;

/// Part of [`RevMeta`](crate::meta::RevMeta) that keeps track of [`PastLenLog`](super::PastLenLog)
/// updates and reports when such an update was expected for a present frame but did not happen.
#[derive(Reflect)]
pub(crate) struct PastLenLogLimits {
    /// Amount of [`PastLenLog`](super::PastLenLog) currently known to this struct
    past_len_ids: AtomicU32,

    /// A channel for [`PastLenLog`](super::PastLenLog) to report their updated limits.
    /// 
    /// Limits per thread are in chronological order, across threads
    /// [`PastLenState::updates_this_frame`] is used to determine draining order. This is only
    /// accurate if the compared limits regard the same log but the order is unimportant for other
    /// comparisions.
    #[reflect(ignore)]
    past_len_updates: Parallel<Vec<PastLenUpdate>>,

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

    // todo
    drain_alloc: Vec<UpdatesLocal<'static>>,

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

/// An iterator over [`PastLenLogLimits::past_len_updates`] that makes sure updates from the same
/// [`PastLenLog`](super::PastLenLog) are yielded chronologically.
struct UpdatesIter<'a>(Vec<UpdatesLocal<'a>>);

/// The per-thread state of the [`UpdatesIter`] iterator.
struct UpdatesLocal<'a> {
    drain: std::vec::Drain<'a, PastLenUpdate>,

    /// The next least-recent update that waits to be picked up by the iterator in which case it is
    /// replaced by the next from [`Self::drain`], if there is one. If not, this state is exhausted.
    next: PastLenUpdate,
}

impl<'a> Iterator for UpdatesIter<'a> {
    type Item = (u32, PastLenLimits);
    fn next(&mut self) -> Option<Self::Item> {
        let (index, local) = self
            .0
            .iter_mut()
            .enumerate()
            .min_by_key(|(_, local)| local.next.state.updates_this_frame)?;

        let next = (local.next.state.id, local.next.limits);

        match local.drain.next() {
            Some(update) => {
                local.next = update;
            }
            None => {
                self.0.swap_remove(index);
            }
        }

        Some(next)
    }
}

impl Debug for PastLenLogLimits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PastLenLimits")
            .field("past_len_ids", &self.past_len_ids)
            .field("past_len_limits", &self.past_len_limits)
            .finish_non_exhaustive()
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
                } else if updated_this_frame_again {
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
        let iter = UpdatesIter(
            self.past_len_updates
                .iter_mut()
                .flat_map(|vec| {
                    let mut drain = vec.drain(..);
                    drain.next().map(|next| UpdatesLocal { drain, next })
                })
                .collect(),
        );
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
        self.past_len_updates
            .borrow_local_mut()
            .push(PastLenUpdate { state, limits });
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn iter_works() {
        fn new_limits(value: u64) -> PastLenLimits {
            PastLenLimits {
                past: value,
                future: value,
                last_update: MaybeLocation::caller(),
            }
        }

        fn new_state(value: u32) -> PastLenState {
            PastLenState {
                id: value,
                updates_this_frame: NonZeroU32::new(value).unwrap(),
                global_log_exits: 0,
                global_log_clears: 0,
            }
        }

        fn new_update(value: u32) -> PastLenUpdate {
            PastLenUpdate {
                state: new_state(value),
                limits: new_limits(value as u64),
            }
        }

        let mut vec1 = vec![new_update(3), new_update(4), new_update(6)];
        let mut vec2 = vec![new_update(4), new_update(5)];
        let iter = UpdatesIter(vec![
            UpdatesLocal {
                drain: vec1.drain(..),
                next: new_update(1),
            },
            UpdatesLocal {
                drain: vec2.drain(..),
                next: new_update(2),
            },
        ]);

        let actual: Vec<_> = iter.collect();
        let expected = vec![
            (1, new_limits(1)),
            (2, new_limits(2)),
            (3, new_limits(3)),
            (4, new_limits(4)),
            (4, new_limits(4)),
            (5, new_limits(5)),
            (6, new_limits(6)),
        ];

        assert_eq!(actual, expected);
    }

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
