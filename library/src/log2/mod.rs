//! This module contains the types around log variants the can be used in reversible systems.
//!
//! # Log variants
//!
//! - [`TransitionLog`], for storing singular values to transition a state forward or backward.
//! - [`TransitionsLog`], for storing multiple values to transition a state forward or backward.
//! - [`UpdateLog`], for keeping track when, how often and with which `max_past_len` value the other
//!   logs need to update in cases these updates happen irregularily. Can also be used as an compact
//!   alternative to `TransitionLog<bool>`.
//!
//! # Optimal log length
//!
//! All logs in an application can sum up to a large amount of data and it is undesired to store any
//! more transition data than what is really needed to cover the [global log length].
//!
//! The transition logs need a `max_past_len` value as a parameter in their `push` and
//! `push_drain_past` methods to determine how many past log entries they should keep to not go
//! [`OutOfLog`] at some point. Depending on how often the log is pushing log entries, the correct
//! source has to be used:
//!
//! | source                                | situation                                             |
//! | ------------------------------------- | ----------------------------------------------------- |
//! | [`RevMeta::past_len`]                 | the log updates at every frame exactly once           |
//! | `RevMeta::past_len * const N`         | the log updates at every frame exactly `N` times      |
//! | [`UpdateLog::push_get_past_len`]      | the log updates arbitrarily                           |
//! | [`UpdateLog::push_many_get_past_len`] | the log updates in varying batches, maybe arbitrarily |
//! | `u64::MAX`                            | the log is allowed to have unlimited growth           |
//!
//! [`UpdateLog`] is able to manage its length from reading the [global log length] from
//! [`RevMeta`].
//!
//! Ideally, as few logs and as few updates as possible are required for the application.
//!
//! # Continuity
//!
//! It is important that the transition logs are updated at the correct frames. This is trivial if
//! they update at every frame exactly once. For other cases, refer to this table:
//!
//! | [`RevDirection`] | method                   | [`RevMeta::now`] |
//! | ---------------- | ------------------------ | ---------------- |
//! | [`NOT_LOG`]      | `push`/`push_drain_past` | `n`              |
//! | [`BackwardLog`]  | `backward_log`           | `n-1`            |
//! | [`FORWARD_LOG`]  | `forward_log`            | `n`              |
//!
//! If a log is updated multiple times per frame, then these amounts must match for these frames as
//! well.
//!
//! As this can become hard to manage, a `UpdateLog` can support these updates by tracking when
//! and how often transition logs need to update. That log also provides the correct value for the
//! `max_past_len` parameter of the pushing methods. See the type documentation of [`UpdateLog`]
//! for examples.
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
//! [id of the `UpdateLog` instance] that was missed.
//!
//! Whenever a [`UpdateLog`] [initializes the internal id], this id is logged at the INFO level with
//! the location in the code. This should help to identify the issue.
//!
//! When bevy's `track_location` cargo feature is active, [`UpdateLogMissed::last_update`] also
//! contains the location where the [`UpdateLog`] was updated the last time.
//!
//! Note however that when a [`RevQueue::Clear`] is applied, all ids until then become invalid. This
//! event is logged at the INFO level as well. Every `UpdateLog` that updates after that will get
//! new ids which is then logged again.
//!
//! ### Example
//!
//! A [`UpdateLog::push_get_past_len`] runs at [frame `42`](crate::meta::RevMeta::now) during
//! [`RevDirection::NOT_LOG`]. This is the first time this log updated. `UpdateLog` will then inform
//! [`RevMeta`] that there is no future frame it expects to run at during
//! [`RevDirection::FORWARD_LOG`] but expects to run at frame `41` when going backward.
//!
//! When then [`UpdateLog::backward_log`] of this specific log runs at `41` during
//! [`RevDirection::BackwardLog`], this gets updated: Now there is no other frame in the past it
//! expects to run at, however it expects to run at frame `42` during [`RevDirection::FORWARD_LOG`].
//!
//! If these updates during the [log directions] do not happen however, [`RevMeta`] will notice
//! that, which triggers the [`UpdateLogsMissed`] error right at the frame where the update was
//! missed. That way the `backward_log`/`forward_log` methods of transition logs that are updated
//! alongside will never fail with [`OutOfLog`].
//!
//!
//! [id of the `UpdateLog` instance]: UpdateLog::id
//! [initializes the internal id]: UpdateLog::pre_update
//! [`RevMeta`]: crate::meta::RevMeta
//! [`RevMeta::now`]: crate::meta::RevMeta::now
//! [`RevMeta::past_len`]: crate::meta::RevMeta::past_len
//! [`RevMeta::update`]: crate::meta::RevMeta::update
//! [global log length]: crate::meta::RevMeta::contains
//! [`RevDirection`]: crate::meta::RevDirection
//! [`RevDirection::NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
//! [`NOT_LOG`]: crate::meta::RevDirection::NOT_LOG
//! [`RevDirection::BackwardLog`]: crate::meta::RevDirection::BackwardLog
//! [`BackwardLog`]: crate::meta::RevDirection::BackwardLog
//! [`RevDirection::FORWARD_LOG`]: crate::meta::RevDirection::FORWARD_LOG
//! [`FORWARD_LOG`]: crate::meta::RevDirection::FORWARD_LOG
//! [log directions]: crate::meta::RevDirection::is_log
//! [`RevQueue::Clear`]: crate::meta::RevQueue::Clear
//! [`RevMetaUpdateErr::UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [`UpdateLogsMissed`]: crate::meta::RevMetaUpdateErr::UpdateLogsMissed
//! [reversible scheduling]: crate::schedule::RevSchedule
//! [reversible commands]: crate::undo_redo::RevCommands

use bevy_ecs::change_detection::MaybeLocation;
use core::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result},
    ops::Range,
};
use std::{
    collections::{VecDeque, vec_deque::Drain},
    iter::FusedIterator,
    usize,
};

//pub(crate) use update::limits::{UpdateLogLimits, UpdateLogState};
//pub use update::{UpdateLog, limits::UpdateLogId, limits::UpdateLogMissed};

pub use transition::{TransitionDrain, TransitionLog};
/*
pub use transitions::{
    LogMut, TransitionsDrainAll, TransitionsDrainChunkable, TransitionsDrainFuture,
    TransitionsDrainPast, TransitionsDrains, TransitionsLog, TransitionsLogIterMut,
    TransitionsLogUpdate,
};
*/
mod transition;
mod transitions;
//mod update;

/// Defines in which way a log has to be adjusted to reflect new changes to
/// [`RevMeta`](crate::meta::RevMeta) since the last time the log was updated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PreUpdateKind {
    /// Keep the log unchanged
    Nothing,

    /// Remove log entries that are in the future
    RemoveFuture,

    /// Remove all log entries.
    RemoveLog,
}

/// An error that may be returned by the `backward_log`/`forward_log` methods of
/// [`TransitionLog`]/[`TransitionsLog`] in case they already were at the end of their log before
/// the method call.
///
/// This error indicates the continuity of the global state was broken.
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
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
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

#[derive(Debug)]
pub struct DrainAll<'a, T, O: Offset> {
    /// A draining iterator of all log entries, if there is a gap ...
    drain: Drain<'a, T>,
    gap_range: GapRange<O>,
    gap_buffer: &'a mut GapBuffer<T>,
}

impl<'a, T, O: Offset> DrainAll<'a, T, O> {
    /// Pick a fitting draining range and mutate `gap` to be `0..deque.len()` when draining is done.
    ///
    /// This way a second call would return `Self` being empty and not draining anything more.
    fn new(
        log: &'a mut VecDeque<T>,
        gap_range: &mut GapRange<O>,
        gap_buffer: &'a mut GapBuffer<T>,
    ) -> Self {
        let mut this_gap = gap_range.clone();

        if gap_range.is_clear() {
            gap_range.start = 0;
            return Self {
                drain: log.drain(..),
                gap_range: this_gap,
                gap_buffer,
            };
        }

        debug_assert!(gap_range.start <= log.len());
        debug_assert!(gap_range.end <= log.len());
        debug_assert!(gap_range.start <= gap_range.end);

        let drain;
        if gap_range.end == log.len() {
            drain = log.drain(..gap_range.start);
            this_gap.end = gap_range.start;
            gap_range.end -= gap_range.start;
            gap_range.start = 0;
        } else if gap_range.start == 0 {
            drain = log.drain(gap_range.end..);
            this_gap.start = 0;
            this_gap.end = 0;
        } else {
            gap_range.start = 0;
            gap_range.end = 0;
            drain = log.drain(..);
        }

        Self {
            drain,
            gap_range: this_gap,
            gap_buffer,
        }
    }
}

impl<T, O: Offset> Iterator for DrainAll<'_, T, O> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if !self.gap_range.is_clear() && self.gap_range.start > 0 {
            let offset = self.gap_range.start_offset.get_offset();
            if self.gap_range.start == offset {
                self.gap_buffer.0 = self.drain.by_ref().take(self.gap_range.end).collect();
                self.gap_range.end -= self.gap_range.start;
                self.gap_range.start = 0;
            } else {
                self.gap_range.start -= 1;
                self.gap_range.end -= 1;
            }
        }
        self.drain.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<T, O: Offset> ExactSizeIterator for DrainAll<'_, T, O> {
    fn len(&self) -> usize {
        if self.gap_range.is_clear() || self.gap_range.start == 0 {
            self.drain.len()
        } else {
            let offset = self.gap_range.start_offset.get_offset();
            self.drain
                .len()
                .saturating_sub(self.gap_range.end - self.gap_range.start + offset)
        }
    }
}

impl<T, O: Offset> FusedIterator for DrainAll<'_, T, O> {}

impl<T, O: Offset> Drop for DrainAll<'_, T, O> {
    fn drop(&mut self) {
        if !self.gap_range.is_clear() && self.gap_range.start > 0 {
            let len = self.gap_range.end - self.gap_range.start;
            self.gap_buffer.0 = self
                .drain
                .by_ref()
                .skip(self.gap_range.start)
                .take(len)
                .collect();
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct GapRange<O> {
    start: usize,
    start_offset: O,
    end: usize,
}

impl<O: Offset> GapRange<O> {
    fn new_clear(index: usize) -> Self {
        Self {
            start: index,
            start_offset: O::default(),
            end: usize::MAX,
        }
    }
    fn is_clear(self) -> bool {
        self.end == usize::MAX
    }
    fn drain_past_end(&mut self) -> usize {
        let end;
        if self.is_clear() {
            end = self.start;
        } else {
            end = self.start.saturating_sub(self.start_offset.get_offset());
            self.end -= end;
        };
        self.start = 0;
        end
    }
    fn drain_future_start(&self) -> usize {
        if self.is_clear() {
            self.start
        } else {
            self.end
        }
    }
}

trait Offset: Copy + Default + Debug {
    fn get_offset(self) -> usize;
}

impl Offset for () {
    fn get_offset(self) -> usize {
        1
    }
}

impl Offset for usize {
    fn get_offset(self) -> usize {
        self
    }
}

#[derive(Debug)]
struct GapBuffer<T>(Box<[T]>);

impl<T> Default for GapBuffer<T> {
    fn default() -> Self {
        Self([].into())
    }
}

impl<T> GapBuffer<T> {
    // todo: prepend https://github.com/rust-lang/rust/issues/146975
    fn prepend_in(&mut self, deque: &mut VecDeque<T>) {
        let mut buffer = core::mem::take(&mut self.0).into_iter();
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
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn repeated_drain_all_keep_deque_unchanged() {
        static EMPTY: &[char] = &[];
        static A: &[char] = &['a'];
        static AB: &[char] = &['a', 'b'];
        static AC: &[char] = &['a', 'c'];
        static ABC: &[char] = &['a', 'b', 'c'];
        static B: &[char] = &['b'];
        static BC: &[char] = &['b', 'c'];
        static C: &[char] = &['c'];

        #[derive(Debug, Clone)]
        struct Test {
            drained: &'static [char],
            kept: &'static [char],
            buffer: &'static [char],
            buffer_drop: &'static [char],
        }

        impl Test {
            fn gap_negative() -> Self {
                Self {
                    drained: ABC,
                    kept: EMPTY,
                    buffer: EMPTY,
                    buffer_drop: EMPTY,
                }
            }
            fn gap_empty(drained: &'static [char], buffer: &'static [char]) -> Self {
                Self {
                    drained,
                    kept: EMPTY,
                    buffer,
                    buffer_drop: EMPTY,
                }
            }
            fn gap_a() -> Self {
                Self {
                    drained: BC,
                    kept: A,
                    buffer: EMPTY,
                    buffer_drop: EMPTY,
                }
            }
            fn gap_b() -> Self {
                Self {
                    drained: C,
                    kept: EMPTY,
                    buffer: AB,
                    buffer_drop: B,
                }
            }
            fn gap_c() -> Self {
                Self {
                    drained: A,
                    kept: C,
                    buffer: B,
                    buffer_drop: EMPTY,
                }
            }
            fn gap_ab() -> Self {
                Self {
                    drained: C,
                    kept: AB,
                    buffer: EMPTY,
                    buffer_drop: EMPTY,
                }
            }
            fn gap_bc() -> Self {
                Self {
                    drained: EMPTY,
                    kept: BC,
                    buffer: A,
                    buffer_drop: EMPTY,
                }
            }
            fn gap_abc() -> Self {
                Self {
                    drained: EMPTY,
                    kept: ABC,
                    buffer: EMPTY,
                    buffer_drop: EMPTY,
                }
            }
        }

        let tests = [
            (0..0, Test::gap_empty(ABC, EMPTY)),
            (0..1, Test::gap_a()),
            (0..2, Test::gap_ab()),
            (0..3, Test::gap_abc()),
            (0..usize::MAX, Test::gap_negative()),
            (1..1, Test::gap_empty(BC, A)),
            (1..2, Test::gap_b()),
            (1..3, Test::gap_bc()),
            (1..usize::MAX, Test::gap_negative()),
            (2..2, Test::gap_empty(AC, B)),
            (2..3, Test::gap_c()),
            (2..usize::MAX, Test::gap_negative()),
            (3..3, Test::gap_empty(AB, C)),
            (3..usize::MAX, Test::gap_negative()),
        ];

        for (i, (gap, test)) in tests.into_iter().enumerate() {
            for collect_first in [false, true] {
                for collect_second in [false, true] {
                    let mut gap_range = GapRange {
                        start: gap.start,
                        start_offset: (),
                        end: gap.end,
                    };
                    let mut deque = ABC.iter().cloned().collect::<VecDeque<_>>();
                    let mut gap_buffer = GapBuffer::default();
                    let drain_all = DrainAll::new(&mut deque, &mut gap_range, &mut gap_buffer);
                    let updated_gap = gap_range.clone();

                    if collect_first {
                        let drained = drain_all.collect::<Vec<_>>();

                        assert_eq!(deque, test.kept, "#{i}");
                        assert_eq!(drained, test.drained, "#{i}");
                        assert_eq!(&*gap_buffer.0, test.buffer, "#{i}");

                        let drain_all = DrainAll::new(&mut deque, &mut gap_range, &mut gap_buffer);

                        if collect_second {
                            let drained = drain_all.collect::<Vec<_>>();
                            assert_eq!(deque, test.kept, "#{i}");
                            assert_eq!(drained, [], "#{i}");
                            assert_eq!(&*gap_buffer.0, test.buffer, "#{i}");
                        } else {
                            let drain_len = drain_all.len();
                            drop(drain_all);

                            assert_eq!(deque, test.kept, "#{i}");
                            assert_eq!(drain_len, 0, "#{i}");
                            assert_eq!(&*gap_buffer.0, test.buffer, "#{i}");
                        }
                    } else {
                        let drain_len = drain_all.len();
                        drop(drain_all);

                        assert_eq!(deque, test.kept, "#{i}");
                        assert_eq!(drain_len, test.drained.len(), "#{i}");
                        assert_eq!(&*gap_buffer.0, test.buffer_drop, "#{i}");

                        let drain_all = DrainAll::new(&mut deque, &mut gap_range, &mut gap_buffer);

                        if collect_second {
                            let drained = drain_all.collect::<Vec<_>>();
                            assert_eq!(deque, test.kept, "#{i}");
                            assert_eq!(drained, [], "#{i}");
                            assert_eq!(&*gap_buffer.0, test.buffer_drop, "#{i}");
                        } else {
                            let drain_len = drain_all.len();
                            drop(drain_all);

                            assert_eq!(deque, test.kept, "#{i}");
                            assert_eq!(drain_len, 0, "#{i}");
                            assert_eq!(&*gap_buffer.0, test.buffer_drop, "#{i}");
                        }
                    }

                    assert_eq!(gap_range.start, updated_gap.start, "#{i}");
                    assert_eq!(gap_range.end, updated_gap.end, "#{i}");
                    gap_buffer.prepend_in(&mut deque);
                    assert!(deque.iter().is_sorted(), "#{i}")
                }
            }
        }
    }
}
