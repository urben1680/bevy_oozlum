//! This module contains [`UpdateLogLimits`] which is part of [`RevMeta`](crate::meta::RevMeta).
//!
//! [`UpdateLog`](super::UpdateLog) interacts with that struct in two ways:
//!
//! 1. Updating [`UpdateLogState`] in the log to determine which [`PreUpdateVariant`] is returned in
//!    [`pre_update`](super::UpdateLog::pre_update).
//! 2. Sending [`UpdateLogLimit`] at
//!    [`push_get_past_len`](super::UpdateLog::push_get_past_len),
//!    [`forward_log`](super::UpdateLog::forward_log) and
//!    [`backward_log`](super::UpdateLog::backward_log). When `RevMeta` is updating
//!   `UpdateLogLimits`, an error can be generated if a `UpdateLog` did not update (often enough)
//!   at the current frame as it did during
//!   [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).

use super::PreUpdateKind;
use bevy_ecs::change_detection::MaybeLocation;
use bevy_utils::Parallel;
use core::{
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    panic::Location,
    sync::atomic::AtomicU32,
};
use nonmax::NonMaxU32;

/// Part of [`RevMeta`](crate::meta::RevMeta) that keeps track of [`UpdateLog`](super::UpdateLog)
/// updates and reports when such an update was expected for a present frame but did not happen.
#[derive(Default)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
pub(crate) struct UpdateLogLimits {
    /// Amount of [`UpdateLog`](super::UpdateLog) currently known to this struct
    past_len_count: AtomicU32,

    /// Amount of times [`update`](Self::update) was called.
    limits_updates: u64,

    /// The locals contain 2D vectors:
    ///
    /// - the outer vector's index represents with which [`UpdateLogState::updates_this_frame`] value
    /// limits were pushed
    /// - the inner vector contains all limits to said `updates_this_frame`
    #[cfg_attr(feature = "bevy_reflect", reflect(ignore))]
    update_log_updates: Parallel<Vec<Vec<(NonMaxU32, UpdateLogLimit)>>>,

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
            .finish_non_exhaustive() // update_log_updates may contain updates for another thread
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
        meta_log_exits: u64,
        meta_log_clears: u64,
    ) -> PreUpdateKind {
        let caller = Location::caller();
        let new_state = || {
            let index = self
                .past_len_count
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let index = NonMaxU32::new(index).expect("exhausted maximum number of `UpdateLog`");
            let state = UpdateLogState {
                index,
                updates_this_frame: 0,
                limits_updates: self.limits_updates,
                meta_log_exits,
                meta_log_clears,
            };
            bevy_log::info!(
                "A `UpdateLog` with the id `{}` was initiated at {caller}, \
                this id remains valid until a `RevQueue::Clear` is applied",
                state.id()
            );
            state
        };
        let Some(state) = state else {
            // init state, no further operation as it starts empty
            *state = Some(new_state());
            return PreUpdateKind::Nothing;
        };
        if state.meta_log_clears < meta_log_clears {
            // meta was cleared in the meantime
            *state = new_state();
            PreUpdateKind::RemoveLog
        } else if state.meta_log_exits < meta_log_exits {
            // meta ran at not-log in the meantime
            state.limits_updates = self.limits_updates;
            state.meta_log_exits = meta_log_exits;
            state.updates_this_frame = 0;
            PreUpdateKind::RemoveFuture
        } else {
            if state.limits_updates != self.limits_updates {
                // this log did not update at this frame yet
                state.limits_updates = self.limits_updates;
                state.updates_this_frame = 0;
            }
            PreUpdateKind::Nothing
        }
    }

    /// Forget all limits and any [`UpdateLog`](super::UpdateLog) that did register so far.
    ///
    /// `UpdateLog`s have to reregister themselves with a new [`UpdateLogState`].
    pub(crate) fn clear(&mut self) {
        self.past_len_count = AtomicU32::new(0);
        self.limits_updates = 0;
        self.update_log_updates
            .iter_mut()
            .flatten()
            .for_each(Vec::clear);
        self.update_log_limits.clear();
    }

    /// Update internal list of [`UpdateLogLimit`] with new updates and check if `now` is breaching
    /// any of the limits.
    pub(crate) fn update(
        &mut self,
        now: u64,
        log_clears: u64,
        log: bool,
    ) -> Result<(), Vec<UpdateLogMissed>> {
        // size up update_log_limits if new `UpdateLog`s updated since the last call of this
        self.update_log_limits.resize(
            *self.past_len_count.get_mut() as usize,
            UpdateLogLimit {
                // in case a new `UpdateLog` called `set_update_state` but not
                // `push_limit`, these bounds need to be infallible
                past: u64::MIN,
                future: u64::MAX,

                // if an error points to this, something went wrong internally
                last_update: MaybeLocation::caller(),
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

        self.limits_updates = self
            .limits_updates
            .checked_add(1)
            .expect("exhausted maximum number of `UpdateLog` updates");

        // there should be no gaps in the 2D vector, otherwise that would be a crate bug
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
            let meta_log_clears = log_clears.to_le_bytes();
            for (index, limits) in self.update_log_limits.iter().enumerate() {
                if now < limits.past || now > limits.future {
                    update_logs_missed.push(UpdateLogMissed {
                        id: UpdateLogId {
                            index: unsafe {
                                // SAFETY: `index` originates from an NonMaxU32 value
                                NonMaxU32::new_unchecked(index as u32)
                            },
                            meta_log_clears,
                        },
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

    /// Update the logged [`UpdateLogLimit`] associated to this `state` with the given `limits`.
    ///
    /// # Panic
    ///
    /// This method panics if `state` is was not updated this frame via
    /// [`UpdateLog::pre_update`](super::UpdateLog::pre_update).
    pub(super) fn push_limit(&self, state: &mut Option<UpdateLogState>, limits: UpdateLogLimit) {
        // get and verify state
        const ERR: &str = "UpdateLog did not run `pre_update` at this reversible frame";
        let state = state.as_mut().expect(ERR);
        assert_eq!(state.limits_updates, self.limits_updates, "{ERR}");

        // push update, extend outer vector if needed
        let v = &mut *self.update_log_updates.borrow_local_mut();
        let index = state.updates_this_frame as usize;
        let new_len = v.len().max(index + 1);
        v.resize(new_len, Vec::new());
        v[index].push((state.index, limits));

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
    /// [`RevMeta`](crate::meta::RevMeta) is [cleared](crate::meta::RevQueue::Clear).
    ///
    /// Is `NonMax` to offer a niche since `Self` is stored in an `Option` at `UpdateLog`.
    index: NonMaxU32,

    /// Counts how many times this [`UpdateLog`](super::UpdateLog) was
    /// [updated](UpdateLogLimits::set_update_state) in this frame.
    updates_this_frame: u32,

    /// Contains the most recent [`UpdateLogLimits::limits_updates`] value that was witnessed by this
    /// [`UpdateLog`](super::UpdateLog).
    limits_updates: u64,

    /// Contains the most recent global count of log exits that was witnessed by this
    /// [`UpdateLog`](super::UpdateLog).
    ///
    /// See [`RevMeta::log_exits`](crate::meta::RevMeta::log_exits).
    meta_log_exits: u64,

    /// Contains the most recent global count of log clears that was witnessed by this
    /// [`UpdateLog`](super::UpdateLog).
    ///
    /// See [`RevMeta::log_clears`](crate::meta::RevMeta::log_clears).
    meta_log_clears: u64,
}

impl UpdateLogState {
    pub(super) fn id(&self) -> UpdateLogId {
        UpdateLogId {
            index: self.index,
            meta_log_clears: self.meta_log_clears.to_ne_bytes(),
        }
    }
}

/// Unique ID of an [`UpdateLog`](super::UpdateLog) from [`RevUpdate::id`](super::UpdateLog::id).
///
/// When [`UpdateLog::pre_update`](super::UpdateLog::pre_update) is called after
/// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) is applied, this id will change for the log.
/// This Id is not reused when outdated.
///
/// The [`Debug`] implementation will return the id as `UpdateLog {int}v{int}` where the first value
/// is [`index`](Self::index) and the second [`meta_log_clears`](Self::meta_log_clears).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UpdateLogId {
    index: NonMaxU32,
    meta_log_clears: [u8; 8],
}

impl UpdateLogId {
    pub(super) const UNINIT: &str = "UpdateLog UNINIT";

    /// The index of this log in [`RevMeta`](crate::meta::RevMeta).
    ///
    /// Other logs with this index may exist at the same time if:
    ///
    /// 1. [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was applied
    /// 2. This log did not update after that
    /// 3. Another log was updated after that
    ///
    /// This log would then get a new id with a unique index at the next
    /// [`UpdateLog::pre_update`](super::UpdateLog::pre_update).
    ///
    /// Only with [`meta_log_clears`](Self::meta_log_clears) this ID is unique.
    pub fn index(self) -> u32 {
        self.index.get()
    }

    /// The latest value of [`RevMeta::log_clears`](crate::meta::RevMeta::log_clears) this log has
    /// witnessed.
    ///
    /// This log will be cleared if this value is outdated and
    /// [`UpdateLog::pre_update`](super::UpdateLog::pre_update) is called, which changes
    /// [`index`](Self::index) as well.
    pub fn meta_log_clears(self) -> u64 {
        u64::from_ne_bytes(self.meta_log_clears)
    }
}

impl Display for UpdateLogId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "UpdateLog {}v{}", self.index(), self.meta_log_clears())
    }
}

impl Debug for UpdateLogId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(self, f)
    }
}

/// A limit for [`RevMeta::now`](crate::meta::RevMeta::now) that, if breached, indicates that a
/// [`UpdateLog`](super::UpdateLog) unexpectedly did not update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "bevy_reflect", derive(bevy_reflect::Reflect))]
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
    /// Will be set to `u64::MAX` at [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG).
    future: u64,

    /// The last location where [`UpdateLog`](super::UpdateLog) was updated. Is empty if bevy's
    /// `track_location` cargo feature is not used.
    last_update: MaybeLocation,
}

impl UpdateLogLimit {
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
    fn next(&mut self) -> Option<Self::Item> {
        match self.pop() {
            Some(index_and_limit) => Some(index_and_limit),
            None => {
                // Try to get an update of the next higher updates_this_frame. If this still yields
                // None then all updates are drained as there would be no gap of updates. A log that
                // updated for the nth time surely updated for the (n-1)th time before if n is > 0.
                // See also the comment in `RevMeta::set_update_state`.
                self.skip_locals = 0;
                self.updates_this_frame += 1;
                self.pop()
            }
        }
    }
}

/// Identifies a [`UpdateLog`](super::UpdateLog) that should have been updated at the current
/// [frame](crate::meta::RevMeta::now) but did not.
///
/// This error can be manually received from [`RevMeta::update`](crate::meta::RevMeta::update). If
/// [`RevMeta::run_rev_update`](crate::meta::RevMeta::run_rev_update) is used, this error, if it
/// occurs, is given to the default error handler.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct UpdateLogMissed {
    /// The internal [id](super::UpdateLog::id) of the [`UpdateLog`](super::UpdateLog).
    pub id: UpdateLogId,

    /// The last location in the code where the [`UpdateLog`](super::UpdateLog) was updated.
    ///
    /// Requires to use bevy's `track_location` feature.
    pub last_update: MaybeLocation,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn updates_state() {
        let mut limits = UpdateLogLimits::default();
        let mut state = None;
        let caller = MaybeLocation::caller();

        // initial set gives Nothing variant
        let variant = limits.set_update_state(&mut state, 0, 0);
        assert_eq!(variant, PreUpdateKind::Nothing);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 0,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );

        // pushing update increases updates_this_frame
        limits.push_limit(&mut state, UpdateLogLimit::new_not_log(1, caller));
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 1,
                limits_updates: 0,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );
        limits.update(2, 0, false).unwrap();

        // update at another frame
        let variant = limits.set_update_state(&mut state, 0, 0);
        assert_eq!(variant, PreUpdateKind::Nothing);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 1,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        limits.push_limit(&mut state, UpdateLogLimit::new_not_log(2, caller));
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 1,
                limits_updates: 1,
                meta_log_exits: 0,
                meta_log_clears: 0,
            })
        );
        limits.update(3, 0, false).unwrap();

        // increased log_exits gives DropFuture variant
        let variant = limits.set_update_state(&mut state, 1, 0);
        assert_eq!(variant, PreUpdateKind::RemoveFuture);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 2,
                meta_log_exits: 1,
                meta_log_clears: 0,
            })
        );

        // update at the same frame increases updates_this_frame again to check next following
        // update to this also resets it again
        limits.push_limit(&mut state, UpdateLogLimit::new_not_log(3, caller));
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 1,
                limits_updates: 2,
                meta_log_exits: 1,
                meta_log_clears: 0,
            })
        );
        limits.update(4, 0, false).unwrap();

        // increased log_clears gived DropLog variant and resets everything
        limits.clear();
        let variant = limits.set_update_state(&mut state, 1, 1);
        assert_eq!(variant, PreUpdateKind::RemoveLog);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ZERO,
                updates_this_frame: 0,
                limits_updates: 0,
                meta_log_exits: 1,
                meta_log_clears: 1,
            })
        );
        limits.update(5, 0, false).unwrap();

        // the same is true if log_exits increased as well in the meantime
        // do not clear counter to demonstrate this issues a new, potentially different id
        // limits.clear();
        let variant = limits.set_update_state(&mut state, 2, 2);
        assert_eq!(variant, PreUpdateKind::RemoveLog);
        assert_eq!(
            state,
            Some(UpdateLogState {
                index: NonMaxU32::ONE,
                updates_this_frame: 0,
                limits_updates: 1,
                meta_log_exits: 2,
                meta_log_clears: 2,
            })
        );
    }

    #[test]
    fn push_and_assert_limits() {
        let mut limits = UpdateLogLimits::default();

        // add a past limit of 1
        let mut past_state = None;
        let past_limit = UpdateLogLimit::new_log(1, u64::MAX, MaybeLocation::caller());
        limits.set_update_state(&mut past_state, 0, 0);
        limits.push_limit(&mut past_state, past_limit);

        // add a future limit of 1
        let mut future_state = None;
        let future_limit = UpdateLogLimit::new_log(u64::MIN, 1, MaybeLocation::caller());
        limits.set_update_state(&mut future_state, 0, 0);
        limits.push_limit(&mut future_state, future_limit);

        // 1 is in both limits
        let result = limits.update(1, 0, true);
        assert_eq!(result, Ok(()));

        // 0 is breaching the past limit
        let result = limits.update(0, 0, true);
        let past_missed = Err(vec![UpdateLogMissed {
            id: past_state.unwrap().id(),
            last_update: past_limit.last_update,
        }]);
        assert_eq!(result, past_missed);

        // 2 is breaching the future limit
        let result = limits.update(2, 0, true);
        let future_missed = Err(vec![UpdateLogMissed {
            id: future_state.unwrap().id(),
            last_update: future_limit.last_update,
        }]);
        assert_eq!(result, future_missed);

        // 1 is in both limits, false unsets future limits
        let result = limits.update(1, 0, false);
        assert_eq!(result, Ok(()));

        // 0 is breaching the past limit
        let result = limits.update(0, 0, true);
        assert_eq!(result, past_missed);

        // 2 is no longer breaching the future limit
        let result = limits.update(2, 0, true);
        assert_eq!(result, Ok(()));
    }
}
