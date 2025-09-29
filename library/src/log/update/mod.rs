use crate::{log::PreUpdateKind, meta::RevMeta};
use bevy_ecs::change_detection::MaybeLocation;
use core::fmt::{Debug, Display};
use std::collections::{TryReserveError, VecDeque};

use limits::*;
use offset::*;

pub(super) mod limits;
mod offset;

/// A log that keeps track when it was updated and provides an alternative value to
/// [`RevMeta::past_len`] for when these updates do not happen exactly once per
/// [`RevUpdate`](crate::schedule::RevUpdate).
///
/// This usually is accompied by another log that would grow too large if `RevMeta::past_len` was
/// used when it actually updates much more rarely. Another use case can be when it runs arbitrarily
/// often per frame and there is no other way to determine how long a log should be when updating.
///
/// If an update is missed, for example when the scope of the log is behind complicated and
/// error-prone scheduling and is just not reached when it should, [`RevMeta::run_rev_update`]
/// will detect this and return an error.
///
/// For examples, see the [`push_get_past_len`](Self::push_get_past_len) method when a single update
/// of an accompied log needs the `max_past_len` value or
/// [`push_many_get_past_len`](Self::push_many_get_past_len) when an accompied log is updated
/// multiple times in a row.
#[derive(Default)]
pub struct UpdateLog {
    /// Offsets that need to be added or subtracted from [`Self::last_update`] to calculate at which
    /// frame the log is expected to be updated.
    ///
    /// For the encoding, see the [`offset`] module.
    offset_bytes: VecDeque<u8>,

    /// A frame this log matched at (or 0) that is either at [`RevMeta::past_end`] or as closely
    /// before it.
    ///
    /// This frame must not be a more recent frame than that because then [`Self::backward_log`]
    /// will be unable to match that frame as [`Self::index`] cannot be reduced further.
    out_of_or_past_end_log: u64,

    /// The chronological last frame in the past this log got updated at.
    last_update: u64,

    /// The current index into [`Self::offset_bytes`]. Always points to the first byte of an
    /// offset sequence or, when at the end of the log, is equal to the `len` of `offset_bytes`.
    index: usize,

    /// The length of the log which is what this log is keeping track of.
    past_len: u64,

    /// The current amount of sequential offsets of `0`.
    zeroes: u8,

    /// The amount of sequential offsets of `0` at the future end of the log.
    zeroes_max: u8,

    /// The state that is needed to get the [`PreUpdateVariant`] at [`Self::pre_update`] and to push
    /// new limits to [`UpdateLogLimits`].
    update_state: Option<UpdateLogState>,
}

impl Debug for UpdateLog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UpdateLog")
            .field("offset_bytes", &self.offset_bytes)
            .field("offsets (decoded)", &OffsetIter(self.offset_bytes.iter()))
            .field("updated at (decoded)", &self.updated_at())
            .field("out_of_or_past_end_log", &self.out_of_or_past_end_log)
            .field("last_update", &self.last_update)
            .field("index", &self.index)
            .field("past_len", &self.past_len)
            .field("zeroes", &self.zeroes)
            .field("zeroes_max", &self.zeroes_max)
            .field("update_state", &self.update_state)
            .finish()
    }
}

impl Display for UpdateLog {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.id() {
            None => write!(f, "UpdateLog #Uninit"),
            Some(id) => write!(f, "UpdateLog #{id} (maybe outdated)"),
        }
    }
}

impl UpdateLog {
    /// Creates an empty log.
    pub const fn new() -> Self {
        Self {
            offset_bytes: VecDeque::new(),
            out_of_or_past_end_log: 0,
            last_update: 0,
            index: 0,
            past_len: 0,
            zeroes: 0,
            zeroes_max: 0,
            update_state: None,
        }
    }

    /// Creates an empty log with space for at least `bytes_capacity` bytes.
    ///
    /// See [`VecDeque::with_capacity`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn with_capacity(bytes_capacity: usize) -> Self {
        Self {
            offset_bytes: VecDeque::with_capacity(bytes_capacity),
            ..Self::new()
        }
    }

    /// Returns the number of bytes in the log.
    ///
    /// See [`VecDeque::len`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn len(&self) -> usize {
        self.offset_bytes.len()
    }

    /// Returns the number of bytes the log can hold without reallocating.
    ///
    /// See [`VecDeque::capacity`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn capacity(&self) -> usize {
        self.offset_bytes.capacity()
    }

    /// Returns `true` if the log contains no bytes.
    ///
    /// See [`VecDeque::is_empty`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn is_empty(&self) -> bool {
        self.offset_bytes.is_empty()
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn reserve(&mut self, additional: usize) {
        self.offset_bytes.reserve(additional)
    }

    /// Reserves capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::reserve_exact`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn reserve_exact(&mut self, additional: usize) {
        self.offset_bytes.reserve_exact(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve(additional)
    }

    /// Tries to reserve capacity for at least `additional` more bytes.
    ///
    /// See [`VecDeque::try_reserve_exact`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.offset_bytes.try_reserve_exact(additional)
    }

    /// Shrinks the capacity of the log with a lower bound.
    ///
    /// See [`VecDeque::shrink_to`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.offset_bytes.shrink_to(min_capacity)
    }

    /// Shrinks the capacity of the log as much as possible.
    ///
    /// See [`VecDeque::shrink_to_fit`].
    ///
    /// Note that the number of bytes has no relation to the length of the log.
    pub fn shrink_to_fit(&mut self) {
        self.offset_bytes.shrink_to_fit()
    }

    /// The internal id of this log, which is only `Some` after it first ran. The id will change
    /// when [`RevQueue::Clear`](crate::meta::RevQueue::Clear) is queued and applied.
    ///
    /// When [`pre_update`](Self::pre_update) initiates the inner state, for example because it is
    /// called for the very first time for this log or
    /// [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was applied, an info log with the id is
    /// written alongside the location where it was called.
    ///
    /// This id is useful to identify missed updates from [`RevMeta::update`]. If
    /// [`RevMeta::run_rev_update`] is used, such errors are handled by the default error handler.
    pub fn id(&self) -> Option<u32> {
        self.update_state.map(|state| state.id.get())
    }

    /// Update the log and return the updated length of the log as an alternative to
    /// [`RevMeta::past_len`].
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG) when the
    /// current scope has been determined for some operation to happen, most often in combination
    /// with another log that is updated next with the returned value.
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) **must** be called at least once in
    /// the [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// For updating this log multiple times during this frame, prefer to use
    /// [`push_many_get_past_len`](Self::push_many_get_past_len) instead.
    ///
    /// # Example
    ///
    /// The following systems reacts on messages. These may not be written for many frames, but then
    /// again they could appear in a large amount in a single frame. One could use a
    /// [`TransitionsLog`](super::TransitionsLog) for that and just push no messages when there are
    /// none. But if this system in turn could also be used with a run condition, then it is
    /// impossible to pick a good `max_past_len` value that makes sure not too many messages are
    /// drained or the log grows way beyond what is needed.
    ///
    /// For that case and other comparable ones, pairing the log with `UpdateLog` fixes this.
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # use bevy_ecs::error::Result;
    /// # use bevy_oozlum::prelude::*;
    /// # #[derive(Message, Clone)]
    /// # struct MyMessage;
    /// fn my_system(
    ///     meta: Res<RevMeta>,
    ///     mut messages: MessageReader<MyMessage>,
    ///     mut update_log: Local<UpdateLog>,
    ///     mut message_log: Local<TransitionsLog<MyMessage>>,
    /// ) -> Result {
    ///     // always call pre_update before further mutations
    ///     update_log.pre_update(&meta);
    ///     message_log.pre_update(&meta);
    ///
    ///     match meta.running_direction() {
    ///         RevDirection::NOT_LOG => {
    ///             if !messages.is_empty() {
    ///                 let past_len = update_log.push_get_past_len(&meta);
    ///                 message_log.push(past_len, |mut log| {
    ///                     for my_message in messages.read() {
    ///                         // use message
    ///
    ///                         log.push(my_message.clone())
    ///                     }
    ///                 })
    ///             }
    ///         }
    ///         RevDirection::FORWARD_LOG => {
    ///             if update_log.forward_log(&meta) {
    ///                 let my_messages = message_log.forward_log()?;
    ///                 for my_message in my_messages {
    ///                     // use message
    ///                 }
    ///             }
    ///         }
    ///         RevDirection::BackwardLog => {
    ///             if update_log.backward_log(&meta) {
    ///                 let my_messages = message_log.backward_log()?;
    ///                 for my_message in my_messages {
    ///                     // use message
    ///                 }
    ///             }
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// ```
    #[track_caller]
    pub fn push_get_past_len(&mut self, meta: &RevMeta) -> u64 {
        self.push_with_caller(meta, MaybeLocation::caller())
    }

    fn push_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> u64 {
        self.truncate_past_and_push_offset(meta);
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_not_log(meta.now(), caller),
        );
        self.past_len
    }

    /// Update the log for `updates` times and return the updated length of the log as an
    /// alternative to [`RevMeta::past_len`].
    ///
    /// Returns `None` if `updates` is zero.
    ///
    /// This is used during [`RevDirection::NOT_LOG`](crate::meta::RevDirection::NOT_LOG) when the
    /// current scope has been determined for some operation to happen, most often in combination
    /// with another log that is updated next with the returned value.
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) **must** be called at least once in
    /// the [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// This method is a more efficient variant of this:
    ///
    /// ```
    /// # use bevy_oozlum::prelude::*;
    /// # let mut meta = RevMeta::default();
    /// # let mut update_log = UpdateLog::new();
    /// # meta.update(|meta_value, _| {
    /// # let meta = &meta_value;
    /// # update_log.pre_update(meta);
    /// # let updates = 10;
    /// let mut past_len = None;
    /// for _ in 0..updates {
    ///     past_len = Some(update_log.push_get_past_len(meta));
    /// }
    /// past_len
    /// # ;
    /// # Some(meta_value)
    /// # }).unwrap();
    /// ```
    ///
    /// # Example
    ///
    /// The following systems reacts on messages. These may not be written for many frames, but then
    /// again they could appear in a large amount in a single frame. One could use a
    /// [`TransitionsLog`](super::TransitionsLog) for that and just push no messages when there are
    /// none. But if this system in turn could also be used with a run condition, then it is
    /// impossible to pick a good `past_len` value that makes sure not too many messages are drained
    /// or the log grows way beyond what is needed.
    ///
    /// ```
    /// # use bevy_ecs::prelude::*;
    /// # use bevy_ecs::error::Result;
    /// # use bevy_oozlum::prelude::*;
    /// # #[derive(Message, Clone)]
    /// # struct MyMessage;
    /// fn my_system(
    ///     meta: Res<RevMeta>,
    ///     mut messages: MessageReader<MyMessage>,
    ///     mut update_log: Local<UpdateLog>,
    ///     mut message_log: Local<TransitionLog<MyMessage>>,
    /// ) -> Result {
    ///     // always call pre_update before further mutations
    ///     update_log.pre_update(&meta);
    ///     message_log.pre_update(&meta);
    ///
    ///     match meta.running_direction() {
    ///         RevDirection::NOT_LOG => {
    ///             // message_log potentially gets updated multiple times
    ///             // this method returns the log len for the last message
    ///             if let Some(past_len) = update_log.push_many_get_past_len(&meta, messages.len() as u64) {
    ///                 for my_message in messages.read() {
    ///                     // use message
    ///
    ///                     message_log.push(past_len, my_message.clone());
    ///                 }
    ///             }
    ///         }
    ///         RevDirection::FORWARD_LOG => {
    ///             let updates = update_log.forward_log_many(&meta);
    ///             for _ in 0..updates {
    ///                 let my_message = message_log.forward_log()?;
    ///                 // use message
    ///             }
    ///         }
    ///         RevDirection::BackwardLog => {
    ///             let updates = update_log.backward_log_many(&meta);
    ///             for _ in 0..updates {
    ///                 let my_message = message_log.backward_log()?;
    ///                 // use message
    ///             }
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub fn push_many_get_past_len(&mut self, meta: &RevMeta, updates: u64) -> Option<u64> {
        self.push_many_with_caller(meta, updates, MaybeLocation::caller())
    }

    fn push_many_with_caller(
        &mut self,
        meta: &RevMeta,
        updates: u64,
        caller: MaybeLocation,
    ) -> Option<u64> {
        if updates == 0 {
            return None;
        }

        // first update
        self.truncate_past_and_push_offset(meta);

        // remaining updates
        let remaining_updates = updates - 1;
        for _ in 0..remaining_updates {
            self.push_zero_offset();
        }
        self.past_len += remaining_updates;
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_not_log(meta.now(), caller),
        );

        Some(self.past_len)
    }

    /// Shortens the log if possible and pushes the present frame.
    ///
    /// Does not verify if [`Self::pre_update`] was called previously this frame as that is done by
    /// [`UpdateLogLimits::push_limit`].
    fn truncate_past_and_push_offset(&mut self, meta: &RevMeta) {
        let iter = OffsetIter(self.offset_bytes.iter());

        let mut to_drain = 0;

        for IterItem { offset, len } in iter {
            if offset == 0 {
                // Offsets of 0 are encoded differently, len is not the amount of bytes,
                // which is actually always 1 here, but the amount of zero offsets in this
                // one byte.
                // We want to get rid of these offsets as well as they dont bring
                // self.out_of_or_past_end_log any closer the limit.
                to_drain += 1;
                self.past_len -= len.get() as u64;
                continue;
            }

            let next_oldest = self.out_of_or_past_end_log + offset;
            if next_oldest > meta.past_end() {
                // next_oldest is reachable by log traversion, which is undesired because
                // Self::backward_log stops working there
                break;
            }

            to_drain += len.get() as usize;
            self.out_of_or_past_end_log = next_oldest;
            self.past_len -= 1;
        }

        self.index -= to_drain;
        // todo: use truncate_front when https://github.com/rust-lang/rust/issues/140667 stabilizes
        self.offset_bytes.drain(..to_drain);

        let offset = meta.now() - self.last_update;
        self.last_update = meta.now();
        self.past_len += 1;

        self.push_offset(offset);
    }

    /// Checks at [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) if this log
    /// has been updated at this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `false` then as well.
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) **must** be called at least once in
    /// the [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// If this log is potenitally updated more than once in the scope this method is used, prefer
    /// [`backward_log_many`](Self::backward_log_many) over using this method in a while loop.
    ///
    /// See the [`update_get`](Self::update_get) for an example.
    #[track_caller]
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        self.backward_log_with_caller(meta, MaybeLocation::caller())
    }

    fn backward_log_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> bool {
        let now_plus_1 = meta.now() + 1;
        // set next backward_limit if this method returns true
        if self.last_update != now_plus_1 {
            return false;
        }
        let backward_limit = if self.zeroes > 0 {
            self.zeroes -= 1;
            now_plus_1
        } else {
            let item = OffsetIter(self.offset_bytes.range(..self.index))
                .next_back()
                .unwrap(); // self.out_of_or_past_end_log should be the last, unreachable log entry
            if item.offset == 0 {
                self.index -= 1;
                self.zeroes = item.len.get() - 1;
                now_plus_1
            } else {
                self.last_update -= item.offset;
                self.index -= item.len.get() as usize;
                self.zeroes = 0;
                self.last_update
            }
        };
        self.past_len -= 1;
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_log(backward_limit, meta.now(), caller),
        );

        true
    }

    /// Checks at [`RevDirection::BackwardLog`](crate::meta::RevDirection::BackwardLog) how many
    /// times this log has been updated at this frame.
    ///
    /// This log is insensitive on checking outside its range of logged frames and just returns
    /// `0` then.
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) **must** be called at least once in
    /// the [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// If this log is updated exactly once in the scope this method is used, prefer
    /// [`backward_log`](Self::backward_log) over this method.
    ///
    /// See [`push_many_get_past_len`](Self::push_many_get_past_len) for an example.
    pub fn backward_log_many(&mut self, meta: &RevMeta) -> u64 {
        self.backward_log_many_with_caller(meta, MaybeLocation::caller())
    }

    fn backward_log_many_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> u64 {
        let now_plus_1 = meta.now() + 1;
        // set next backward_limit if this method returns true
        if self.last_update != now_plus_1 {
            return 0;
        }
        let iter = OffsetIter(self.offset_bytes.range(..self.index));
        let mut updates = self.zeroes as u64 + 1;
        for item in iter.rev() {
            if item.offset != 0 {
                self.last_update -= item.offset;
                self.index -= item.len.get() as usize;
                self.zeroes = 0;
                break;
            }
            self.index -= 1;
            self.zeroes = 0;
            updates += item.len.get() as u64;
        }
        self.past_len -= updates;
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_log(self.last_update, meta.now(), caller),
        );

        updates
    }

    /// Checks at [`RevDirection::FORWARD_LOG`](crate::meta::RevDirection::FORWARD_LOG) if this log
    /// has been updated at this frame.
    ///
    /// Returns `true` if that is the case or `false` if not. This log is insensitive on
    /// checking outside its range of logged frames and just returns `false` then as well.
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) **must** be called at least once in
    /// the [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// If this log is potenitally updated more than once in the scope this method is used, prefer
    /// [`forward_log_many`](Self::forward_log_many) over using this method in a while loop.
    ///
    /// See [`update_get`](Self::update_get) for an example.
    #[track_caller]
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        self.forward_log_with_caller(meta, MaybeLocation::caller())
    }

    fn forward_log_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> bool {
        let now_minus_1 = meta.now() - 1;
        // set next forward_limit if this method returns true
        let mut iter = OffsetIter(self.offset_bytes.range(self.index..));
        let forward_limit = match iter.next() {
            Some(IterItem { offset: 0, len }) => {
                if self.last_update != meta.now() {
                    return false;
                }
                if self.zeroes < len.get() as u8 - 1 {
                    self.zeroes += 1;
                    now_minus_1
                } else {
                    self.index += 1;
                    self.zeroes = 0;
                    match iter.next() {
                        Some(IterItem { offset, .. }) => now_minus_1 + offset,
                        None if self.zeroes_max == 0 => u64::MAX,
                        None => now_minus_1,
                    }
                }
            }
            Some(IterItem { offset, len }) => {
                let frame = self.last_update + offset;
                if frame != meta.now() {
                    return false;
                }
                self.last_update = frame;
                self.index += len.get() as usize;
                self.zeroes = 0;
                match iter.next() {
                    Some(IterItem { offset, .. }) => frame - 1 + offset,
                    None if self.zeroes_max == 0 => u64::MAX,
                    None => frame - 1,
                }
            }
            None if self.zeroes < self.zeroes_max => {
                if self.last_update != meta.now() {
                    return false;
                }
                self.zeroes += 1;
                if self.zeroes == self.zeroes_max {
                    u64::MAX
                } else {
                    now_minus_1
                }
            }
            // reached end of log
            None => return false,
        };
        self.past_len += 1;
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_log(meta.now(), forward_limit, caller),
        );
        true
    }

    /// Checks at [`RevDirection::FORWARD_LOG`](crate::meta::RevDirection::FORWARD_LOG) how many
    /// times this log has been updated at this frame.
    ///
    /// This log is insensitive on checking outside its range of logged frames and just returns
    /// `0` then.
    ///
    /// Before calling this, [`pre_update`](Self::pre_update) **must** be called at least once in
    /// the [present reversible frame](RevMeta::now). This method may panic if this was not done.
    ///
    /// If this log is updated exactly once in the scope this method is used, prefer
    /// [`forward_log`](Self::forward_log) over this method.
    ///
    /// See [`push_many_get_past_len`](Self::push_many_get_past_len) for an example.
    pub fn forward_log_many(&mut self, meta: &RevMeta) -> u64 {
        self.forward_log_many_with_caller(meta, MaybeLocation::caller())
    }

    fn forward_log_many_with_caller(&mut self, meta: &RevMeta, caller: MaybeLocation) -> u64 {
        let now_minus_1 = meta.now() - 1;
        // set next forward_limit if this method returns true
        let mut iter = OffsetIter(self.offset_bytes.range(self.index..));
        let mut updates = 0;
        let mut forward_limit = now_minus_1;
        loop {
            match iter.next() {
                Some(IterItem { offset: 0, len }) => {
                    if self.last_update != meta.now() {
                        break;
                    }
                    updates += (len.get() - self.zeroes) as u64;
                    self.index += 1;
                    self.zeroes = 0;
                    forward_limit = match iter.clone().next() {
                        Some(IterItem { offset, .. }) => now_minus_1 + offset,
                        None if self.zeroes_max == 0 => u64::MAX,
                        None => now_minus_1,
                    };
                }
                Some(IterItem { offset, len }) => {
                    let frame = self.last_update + offset;
                    if frame != meta.now() {
                        break;
                    }
                    self.last_update = frame;
                    self.index += len.get() as usize;
                    self.zeroes = 0;
                    updates += 1;
                    forward_limit = match iter.clone().next() {
                        Some(IterItem { offset, .. }) => frame - 1 + offset,
                        None if self.zeroes_max == 0 => u64::MAX,
                        None => frame - 1,
                    };
                }
                None => {
                    if self.last_update == meta.now() && self.zeroes < self.zeroes_max {
                        updates += (self.zeroes_max - self.zeroes) as u64;
                        self.zeroes = self.zeroes_max;
                        forward_limit = u64::MAX;
                    }
                    break;
                }
            }
        }
        if updates == 0 {
            return 0;
        }
        self.past_len += updates;
        meta.update_log_limits().push_limit(
            &mut self.update_state,
            UpdateLogLimit::new_log(meta.now(), forward_limit, caller),
        );
        updates
    }

    /// This method **must** be called at least once per [reversible frame](RevMeta::now) before
    /// updating this log further.
    ///
    /// This method has no effect if called again in the same frame.
    ///
    /// If this initiates the inner state, for example because this is called for the very first
    /// time for this log or [`RevQueue::Clear`](crate::meta::RevQueue::Clear) was applied, an info
    /// log with the [`id`](Self::id) is written alongside the location where this was called.
    #[track_caller]
    pub fn pre_update(&mut self, meta: &RevMeta) {
        match meta.set_update_state(&mut self.update_state) {
            PreUpdateKind::RemoveLog => {
                self.offset_bytes.clear();
                self.out_of_or_past_end_log = 0;
                self.last_update = 0;
                self.index = 0;
                self.past_len = 0;
                self.zeroes = 0;
                self.zeroes_max = 0;
            }
            PreUpdateKind::RemoveFuture => {
                if self.offset_bytes.len() > self.index {
                    self.offset_bytes.truncate(self.index);
                    self.zeroes = 0;
                }
                self.zeroes_max = self.zeroes;
            }
            PreUpdateKind::Nothing => {}
        }
    }
}

#[cfg(test)]
mod test {
    use crate::meta::{RevDirection, RevQueue};

    use super::*;

    struct MetaAndLog {
        meta: RevMeta,
        update_log: UpdateLog,
        last_update: MaybeLocation,
    }

    impl MetaAndLog {
        fn new(max_world_states: u64) -> Self {
            Self {
                meta: RevMeta::new(core::num::NonZeroU64::new(max_world_states), false),
                update_log: UpdateLog::new(),
                last_update: MaybeLocation::caller(),
            }
        }
        #[track_caller]
        fn forward<const N: usize>(&mut self, past_lens: [u64; N], clear: bool) {
            let caller = MaybeLocation::caller();
            let queue = if clear {
                RevQueue::CLEAR_THEN_RUN
            } else {
                RevQueue::RUN_NOT_LOG
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::NOT_LOG);
                self.update_log.pre_update(meta);
                for past_len in past_lens {
                    assert_eq!(
                        self.update_log.push_with_caller(meta, caller),
                        past_len,
                        "{:#?}",
                        self.update_log
                    );
                    self.last_update = caller;
                }
            });
        }
        #[track_caller]
        fn forward_log(&mut self, updates: u64) {
            let caller = MaybeLocation::caller();

            // test cases where not all updates ran
            if updates > 0 {
                let mut missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    if insufficient_updates == 1 {
                        missed.last_update = caller;
                    }
                    self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::FORWARD_LOG);
                        self.update_log.pre_update(meta);
                        for _ in 0..insufficient_updates {
                            assert_eq!(self.update_log.forward_log_with_caller(meta, caller), true);
                        }
                    });
                    self.revert(insufficient_updates, false);
                }
            }

            // test cases where all updates ran

            // test one-by-one updates
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::FORWARD_LOG);
                self.update_log.pre_update(meta);
                for _ in 0..updates {
                    assert!(self.update_log.forward_log_with_caller(meta, caller));
                }
                // assert no more updates would run
                assert_eq!(self.update_log.forward_log(meta), false);
                assert_eq!(self.update_log.forward_log_many(meta), 0);
            });
            self.revert(updates, false);

            // test test many updates
            self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::FORWARD_LOG);
                self.update_log.pre_update(meta);
                let actual = self.update_log.forward_log_many_with_caller(meta, caller);
                assert_eq!(actual, updates, "after many: {:#?}", self.update_log);
                // assert no more updates would run
                assert_eq!(self.update_log.forward_log(meta), false);
                assert_eq!(self.update_log.forward_log_many(meta), 0);
            });

            if updates != 0 {
                self.revert(updates, false);

                // test many updates after a single update
                self.meta.set_queue(RevQueue::RUN_FORWARD_LOG);
                self.meta.update_ref(Ok(true), |meta, direction| {
                    assert_eq!(direction, RevDirection::FORWARD_LOG);
                    self.update_log.pre_update(meta);
                    assert!(self.update_log.forward_log_with_caller(meta, caller));
                    let actual = self.update_log.forward_log_many_with_caller(meta, caller);
                    assert_eq!(actual + 1, updates);
                    // assert no more updates would run
                    assert_eq!(self.update_log.forward_log(meta), false);
                    assert_eq!(self.update_log.forward_log_many(meta), 0);
                });

                self.last_update = caller;
            }
        }
        #[track_caller]
        fn backward_log(&mut self, updates: u64) {
            let caller = MaybeLocation::caller();

            // test cases where not all updates ran
            if updates > 0 {
                let mut missed = self.new_missed();

                for insufficient_updates in 0..updates {
                    if insufficient_updates == 1 {
                        missed.last_update = caller;
                    }
                    self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
                    self.meta.update_ref(Err(missed), |meta, direction| {
                        assert_eq!(direction, RevDirection::BackwardLog);
                        self.update_log.pre_update(meta);
                        for _ in 0..insufficient_updates {
                            assert_eq!(
                                self.update_log.backward_log_with_caller(meta, caller),
                                true
                            );
                        }
                    });
                    self.revert(insufficient_updates, true);
                }
            }

            // test cases where all updates ran

            // test one-by-one updates
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::BackwardLog);
                self.update_log.pre_update(meta);
                for _ in 0..updates {
                    assert!(self.update_log.backward_log_with_caller(meta, caller));
                }
                // assert no more updates would run
                assert_eq!(self.update_log.backward_log(meta), false);
                assert_eq!(self.update_log.backward_log_many(meta), 0);
            });
            self.revert(updates, true);

            // test test many updates
            self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
            self.meta.update_ref(Ok(true), |meta, direction| {
                assert_eq!(direction, RevDirection::BackwardLog);
                self.update_log.pre_update(meta);
                let actual = self.update_log.backward_log_many_with_caller(meta, caller);
                assert_eq!(actual, updates);
                // assert no more updates would run
                assert_eq!(self.update_log.backward_log(meta), false);
                assert_eq!(self.update_log.backward_log_many(meta), 0);
            });

            if updates != 0 {
                self.revert(updates, true);

                // test many updates after a single update
                self.meta.set_queue(RevQueue::RUN_BACKWARD_LOG);
                self.meta.update_ref(Ok(true), |meta, direction| {
                    assert_eq!(direction, RevDirection::BackwardLog);
                    self.update_log.pre_update(meta);
                    assert!(self.update_log.backward_log_with_caller(meta, caller));
                    let actual = self.update_log.backward_log_many_with_caller(meta, caller);
                    assert_eq!(actual + 1, updates);
                    // assert no more updates would run
                    assert_eq!(self.update_log.backward_log(meta), false);
                    assert_eq!(self.update_log.backward_log_many(meta), 0);
                });

                self.last_update = caller;
            }
        }
        fn new_missed(&self) -> UpdateLogMissed {
            UpdateLogMissed {
                id: self.update_log.update_state.unwrap().id.get(),
                last_update: self.last_update,
            }
        }
        fn revert(&mut self, updates: u64, forward: bool) {
            let queue = if forward {
                RevQueue::RUN_FORWARD_LOG
            } else {
                RevQueue::RUN_BACKWARD_LOG
            };
            self.meta.set_queue(queue);
            self.meta.update_ref(Ok(true), |meta, _| {
                self.update_log.pre_update(meta);
                if forward {
                    for _ in 0..updates {
                        assert_eq!(
                            self.update_log
                                .forward_log_with_caller(meta, self.last_update),
                            true,
                        );
                    }
                } else {
                    for _ in 0..updates {
                        assert_eq!(
                            self.update_log
                                .backward_log_with_caller(meta, self.last_update),
                            true,
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut meta_and_log = MetaAndLog::new(4);

        meta_and_log.forward([1], false); // frame #1
        meta_and_log.forward([2, 3], false); // frame #2
        meta_and_log.forward([4, 5], false);
        meta_and_log.forward([], false);
        // shortened log of runs from frame #1 and #2 --> past_len -= 3
        meta_and_log.forward([3, 4, 5], false);

        meta_and_log.backward_log(3);
        meta_and_log.backward_log(0);
        meta_and_log.backward_log(2);

        meta_and_log.forward_log(2);
        meta_and_log.forward_log(0);
        meta_and_log.forward_log(3);

        meta_and_log.backward_log(3);
        meta_and_log.backward_log(0);

        meta_and_log.forward([3], false);

        meta_and_log.backward_log(1);

        meta_and_log.forward([], false); // should unset future limit
        meta_and_log.forward([], false);

        meta_and_log.backward_log(0);
        meta_and_log.backward_log(0);

        meta_and_log.forward_log(0);
        meta_and_log.forward_log(0);

        meta_and_log.forward([1, 2], false);
        meta_and_log.forward([1, 2], true);

        meta_and_log.backward_log(2);

        meta_and_log.forward_log(2);
    }

    #[test]
    fn behaves_like_meta_if_updated_once_per_frame() {
        let mut meta_and_log = MetaAndLog::new(4);

        meta_and_log.forward([1], false);
        assert_eq!(meta_and_log.meta.past_len(), 1);

        meta_and_log.forward([2], false);
        assert_eq!(meta_and_log.meta.past_len(), 2);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);

        meta_and_log.forward([3], false);
        assert_eq!(meta_and_log.meta.past_len(), 3);
    }
}
