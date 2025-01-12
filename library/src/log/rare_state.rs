use core::fmt::Debug;
use std::{
    collections::{TryReserveError, VecDeque},
    ops::Deref,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::{index_oob, partition_point, LoggedAt, OutOfLog, RareDrain, RareValue};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareStateLog<T> {
    /// RareValue.skips represents the number of None pushes after the state in the struct
    states: VecDeque<RareValue<T>>,
    present: T,
    index: usize,
    skips: usize,
    skips_max: usize,
    past_len: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{
        LoglessState, LoglessWithCapacity, WithCapacity, WithCapacityWrapper,
    };

    use super::{RareStateLog, RareValue};

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessState for RareStateLog<T> {
        type Se<'se> = &'se T;
        type De = T;
        fn get_logless_state(&self) -> Self::Se<'_> {
            &self.present
        }
        fn from_logless_state(logless_state: Self::De) -> Self {
            logless_state.into()
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for RareStateLog<T> {
        type Se<'se> = (
            WithCapacityWrapper<&'se VecDeque<RareValue<T>>>,
            &'se T,
            usize,
            usize,
            usize,
            usize,
        );
        type De = (
            WithCapacityWrapper<VecDeque<RareValue<T>>>,
            T,
            usize,
            usize,
            usize,
            usize,
        );
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                WithCapacityWrapper(&self.states),
                &self.present,
                self.index,
                self.skips,
                self.skips_max,
                self.past_len,
            )
        }
        fn from_with_capacity(
            (WithCapacityWrapper(states), present, index, skips, skips_max, past_len): Self::De,
        ) -> Self {
            Self {
                states,
                present,
                index,
                skips,
                skips_max,
                past_len,
            }
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessWithCapacity for RareStateLog<T> {
        type Se<'se> = (&'se T, usize);
        type De = (T, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (&self.present, self.states_capacity())
        }
        fn from_logless_with_capacity((present, states_capacity): Self::De) -> Self {
            Self::with_capacity(present, states_capacity)
        }
    }
}

impl<T> From<T> for RareStateLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> Deref for RareStateLog<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.present
    }
}

impl<T> RareStateLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present,
            index: 0,
            skips: 0,
            skips_max: 0,
            past_len: 0,
        }
    }
    pub fn with_capacity(present: T, states_capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(states_capacity),
            ..Self::new(present)
        }
    }
    pub fn into_inner(self) -> T {
        self.present
    }
    pub fn states_len(&self) -> usize {
        self.states.len()
    }
    pub fn states_capacity(&self) -> usize {
        self.states.capacity()
    }
    pub fn states_is_empty(&self) -> bool {
        self.states.is_empty()
    }
    pub fn states_reserve(&mut self, additional: usize) {
        self.states.reserve(additional)
    }
    pub fn states_reserve_exact(&mut self, additional: usize) {
        self.states.reserve_exact(additional)
    }
    pub fn states_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve(additional)
    }
    pub fn states_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.states.try_reserve_exact(additional)
    }
    pub fn states_shrink_to(&mut self, min_capacity: usize) {
        self.states.shrink_to(min_capacity)
    }
    pub fn states_shrink_to_fit(&mut self) {
        self.states.shrink_to_fit()
    }
    pub fn drain_future(&mut self) -> RareDrain<T> {
        RareDrain(self.states.drain(self.index..))
    }
    pub fn clear(&mut self) {
        self.states.clear();
        self.index = 0;
        self.skips = 0;
        self.skips_max = 0;
        self.past_len = 0;
    }
    pub fn clear_with(&mut self, present: T) {
        self.present = present;
        self.clear();
    }
    pub fn push_present(&mut self, state: Option<T>) {
        self.states.truncate(self.index);
        match state {
            None => {
                self.skips += 1;
                self.past_len += 1;
            }
            Some(state) => {
                let previous = core::mem::replace(&mut self.present, state);
                self.states.push_back(RareValue::new(previous, self.skips));
                self.skips = 0;
                self.index += 1;
                self.past_len += 1;
            }
        }
        self.skips_max = self.skips;
    }
    pub fn backward_log(&mut self) -> Result<bool, OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.past_len -= 1;
            return Ok(false);
        }
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        if !self.swap_state_and_skips_max(index) {
            return Err(index_oob());
        }
        self.index = index;
        self.skips = self.skips_max;
        self.past_len -= 1;
        Ok(true)
    }
    pub fn forward_log(&mut self) -> Result<bool, OutOfLog> {
        if self.skips < self.skips_max {
            self.past_len += 1;
            self.skips += 1;
            Ok(false)
        } else if self.swap_state_and_skips_max(self.index) {
            self.past_len += 1;
            self.index += 1;
            self.skips = 0;
            Ok(true)
        } else {
            Err(OutOfLog)
        }
    }
    fn swap_state_and_skips_max(&mut self, index: usize) -> bool {
        self.states
            .get_mut(index)
            .map(|entry| {
                let mut skips_max = self.skips_max.to_ne_bytes();
                core::mem::swap(&mut self.present, &mut entry.value);
                core::mem::swap(&mut skips_max, &mut entry.skips_ne);
                self.skips_max = usize::from_ne_bytes(skips_max);
            })
            .is_some()
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        if self.index == 0 {
            return None;
        }
        let excessive_len = self
            .past_len
            .checked_sub(max_past_len)
            .filter(|len| *len > 0)?;
        let past_end = self.states.front()?;
        if excessive_len >= past_end.len() {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> RareDrain<T> {
        let mut drain_amount = 0;
        for entry in self.states.iter() {
            let less = self.past_len - entry.len();
            if less < max_past_len {
                break;
            }
            self.past_len = less;
            drain_amount += 1;
        }
        self.index -= drain_amount;
        RareDrain(self.states.drain(..drain_amount))
    }
    fn pop_past(&mut self) -> Option<T> {
        self.states.pop_front().map(|rare| {
            self.index -= 1;
            self.past_len -= rare.len();
            rare.value
        })
    }
}

impl<T: LoggedAt> RareStateLog<T> {
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<T> {
        if self.index == 0 {
            return None;
        }
        // The user might call `push_present` multiple times per frame, so `RareValue::skips`
        // cannot be reliably interpreted as frame offsets from `RareValue::logged_at`.
        // Instead `RareValue::skips` is ignored here and pop only happen if the next entry is out of log too.
        // It might be possible to ignore this issue if the oldest entry has no skips,
        // but simplicity is favored here to keep lookups, operations and maintenance burden lower.
        let logged_at = self.states.get(1)?.logged_at();
        if !meta.contains_in_past(logged_at, true, true) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_logged_at(&mut self, meta: &RevMeta) -> RareDrain<T> {
        // The user might call `push_present` multiple times per frame, so `RareValue::skips`
        // cannot be reliably interpreted as frame offsets from `RareValue::logged_at`.
        // Instead `RareValue::skips` is ignored here and one entry less is removed.
        // It might be possible to ignore this issue if the entry at the partition point has no skips,
        // but simplicity is favored here to keep lookups, operations and maintenance burden lower.
        let to = partition_point(&self.states, self.index, meta).saturating_sub(1);
        self.past_len -= to // `to` plus sum of `RareValue::skips` == sum of `RareValue::len` but with less operations 
            + self
                .states
                .range(..to)
                .map(RareValue::skips)
                .sum::<usize>();
        self.index -= to;

        RareDrain(self.states.drain(..to))
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU32;

    use serde::{Deserialize, Serialize};

    use crate::{
        frame::RevFrame,
        log::test::{shorten_strategy, ShortenStrategy},
    };

    use super::*;

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: RareStateLog<char>,
            #[serde(with = "crate::log::logless_state")]
            logless: RareStateLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: RareStateLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: RareStateLog<char>,
        }

        let mut original = RareStateLog::from('a');
        original.push_present(Some('b'));
        original.push_present(Some('c'));
        original.backward_log().expect("in log");

        let mut logs = Logs {
            full: original.clone(),
            logless: original.clone(),
            full_with_capacity: original.clone(),
            logless_with_capacity: original.clone(),
        };

        logs.full.states_reserve_exact(98);
        logs.logless.states_reserve_exact(98);
        logs.full_with_capacity.states_reserve_exact(98);
        logs.logless_with_capacity.states_reserve_exact(98);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            full,
            logless,
            full_with_capacity,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &RareStateLog<char>, len, with_capacity| {
            assert_eq!(
                **log, 'b',
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.states_len(),
                len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.states_capacity() >= 100,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.states_capacity()
            );
        };

        test(&full, 2, false);
        test(&logless, 0, false);
        test(&full_with_capacity, 2, true);
        test(&logless_with_capacity, 0, true);
    }

    #[test]
    fn clear() {
        let mut original = RareStateLog::new(1);
        original.push_present(Some(2));
        original.push_present(None);
        original.push_present(Some(3));
        original.backward_log().expect("in log");

        let mut log = original.clone();
        log.clear();
        assert_eq!(*log, 2, "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );

        let mut log = original.clone();
        log.clear_with(4);
        assert_eq!(*log, 4, "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );
    }

    impl RareStateLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            max_past_len: usize, // control when the by-len strategies trigger pop/drain to align to the by-logged-at strategies
            state: (u8, u32),
            state_is_pushed: bool,
            expected_states_len: usize,
            expected_popped: Option<(u8, u32)>,
        ) {
            meta.queue_forward();
            meta.update(|_, _| {});
            let before = self.clone();
            let push = state_is_pushed.then(|| (state.0, RevFrame::checked_new(state.1)));
            self.push_present(push);
            let after_push = self.clone();
            let actual_popped =
                shorten_strategy!(self, meta, strategy, max_past_len, before, after_push);
            assert_eq!(
                actual_popped, expected_popped,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            let actual_states_len = self.states_len();
            assert_eq!(
                actual_states_len, expected_states_len,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            self.test_state(before, meta, state);
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: (u8, u32),
            expected_result: Result<bool, OutOfLog>,
        ) {
            let before = self.clone();
            let actual_result = self.forward_log();
            if expected_result.is_ok() {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
            }
            assert_eq!(
                actual_result, expected_result,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            self.test_state(before, meta, expected_state);
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: (u8, u32),
            expected_result: Result<bool, OutOfLog>,
        ) {
            let before = self.clone();
            let actual_result = self.backward_log();
            if expected_result.is_ok() {
                let frame = meta.present_world_state().wrapping_sub(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
            }
            assert_eq!(
                actual_result, expected_result,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            self.test_state(before, meta, expected_state);
        }
        fn test_state(&self, before: Self, meta: &RevMeta, state: (u8, u32)) {
            let state = (state.0, RevFrame::checked_new(state.1));
            assert_eq!(
                **self, state,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (u8, u32)>,
            expected_states_len: usize,
        ) -> Self {
            let before = self.clone();
            let mut clone = self.clone();
            let actual_future: Vec<_> = clone.drain_future().collect();
            let expected_future: Vec<_> = expected_future
                .into_iter()
                .map(|(state, frame)| (state, RevFrame(frame)))
                .collect();
            assert_eq!(
                actual_future, expected_future,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            assert_eq!(
                clone.states_len(),
                expected_states_len,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut log = RareStateLog::new((0, meta.present_world_state()));

            log.test_forward(meta, strategy, 0, (0, 0), false, 0, None);
            log.test_forward(meta, strategy, 1, (2, 2), true, 1, None);
            // does not pop yet because the skip after the initial state is still in log range
            log.test_forward(meta, strategy, 2, (3, 3), true, 2, None);
            // does not pop yet because while the skip after the initial state is no longer in log range,
            // multiple uses of `push_present` by the user at the same frame makes the skips as frame offset
            // an unreliable indicator to pop here
            log.test_forward(meta, strategy, 3, (3, 3), false, 2, None);
            // pops oldest entry as the second-oldest entry is also out of log now
            log.test_forward(meta, strategy, 3, (5, 5), true, 2, Some((0, 0)));

            meta.set_oldest_frame(1); // make log start accessible again to test out-of-log

            log.test_backward_log(meta, (3, 3), Ok(true));
            log.test_backward_log(meta, (3, 3), Ok(false));
            log.test_backward_log(meta, (2, 2), Ok(true));
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, (2, 2), Err(OutOfLog));

            log.test_forward_log(meta, (3, 3), Ok(true));
            log.test_forward_log(meta, (3, 3), Ok(false));
            log.test_forward_log(meta, (5, 5), Ok(true));
            // out of log, no mutations happend to both meta and log here
            log.test_forward_log(meta, (5, 5), Err(OutOfLog));

            log.test_backward_log(meta, (3, 3), Ok(true));
            log.test_backward_log(meta, (3, 3), Ok(false));
            log.test_backward_log(meta, (2, 2), Ok(true));

            let log_clone = log.test_drain_future([(3, 3), (5, 5)], 0);

            for mut log in [log, log_clone] {
                // all entries are truncated as they are in the future
                log.test_forward(meta, strategy, 3, (4, 3), true, 1, None);
            }
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareStateLog<RevFrame>>();
    }
}
