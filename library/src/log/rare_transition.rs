use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

use crate::meta::RevMeta;

use super::{index_oob, LoggedAt, OutOfLog, RareDrain, RareValue};

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareTransitionLog<T> {
    /// RareValue.skips represents the number of None pushes before the transition in the struct.
    transitions: VecDeque<RareValue<T>>,
    index: usize,
    /// For simplicity, this never gets reduced by `pop`/`drain_past_by_len`/`logged_at`.
    skips: usize,
    /// Used to check for OutOfLog error when calling `self.forward_log`/`logged_at`.
    ///
    /// For simplicity, this never gets reduced by `pop`/`drain_past_by_len`.
    skips_max: usize,
    past_len: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::{RareTransitionLog, RareValue};

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for RareTransitionLog<T> {
        type Se<'se> = (
            WithCapacityWrapper<&'se VecDeque<RareValue<T>>>,
            usize,
            usize,
            usize,
            usize,
        );
        type De = (
            WithCapacityWrapper<VecDeque<RareValue<T>>>,
            usize,
            usize,
            usize,
            usize,
        );
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                WithCapacityWrapper(&self.transitions),
                self.index,
                self.skips,
                self.skips_max,
                self.past_len,
            )
        }
        fn from_with_capacity(
            (WithCapacityWrapper(transitions), index, skips, skips_max, past_len): Self::De,
        ) -> Self {
            Self {
                transitions,
                index,
                skips,
                skips_max,
                past_len,
            }
        }
    }

    impl<T> LoglessWithCapacity for RareTransitionLog<T> {
        type Se<'se> = usize where T: 'se;
        type De = usize;
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            self.transitions.capacity()
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
    }
}

impl<T> Default for RareTransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> RareTransitionLog<T> {
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            index: 0,
            skips: 0,
            skips_max: 0,
            past_len: 0,
        }
    }
    pub fn with_capacity(transitions_capacity: usize) -> Self {
        Self {
            transitions: VecDeque::with_capacity(transitions_capacity),
            index: 0,
            skips: 0,
            skips_max: 0,
            past_len: 0,
        }
    }
    pub fn transitions_len(&self) -> usize {
        self.transitions.len()
    }
    pub fn transitions_capacity(&self) -> usize {
        self.transitions.capacity()
    }
    pub fn transitions_is_empty(&self) -> bool {
        self.transitions.is_empty()
    }
    pub fn transitions_reserve(&mut self, additional: usize) {
        self.transitions.reserve(additional)
    }
    pub fn transitions_reserve_exact(&mut self, additional: usize) {
        self.transitions.reserve_exact(additional)
    }
    pub fn transitions_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.transitions.try_reserve(additional)
    }
    pub fn transitions_try_reserve_exact(
        &mut self,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        self.transitions.try_reserve_exact(additional)
    }
    pub fn transitions_shrink_to(&mut self, min_capacity: usize) {
        self.transitions.shrink_to(min_capacity)
    }
    pub fn transitions_shrink_to_fit(&mut self) {
        self.transitions.shrink_to_fit()
    }
    pub fn drain_future(&mut self) -> RareDrain<T> {
        self.skips_max = self.skips;
        RareDrain(self.transitions.drain(self.index..))
    }
    pub fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
        self.past_len = 0;
        self.skips = 0;
        self.skips_max = 0;
    }
    pub fn push_present(&mut self, transition: Option<T>) {
        self.transitions.truncate(self.index);
        match transition {
            None => {
                self.skips += 1;
                self.past_len += 1;
            }
            Some(transition) => {
                self.transitions
                    .push_back(RareValue::new(transition, self.skips));
                self.index += 1;
                self.skips = 0;
                self.past_len += 1;
            }
        }
        self.skips_max = self.skips;
    }
    pub fn backward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.past_len -= 1;
            Ok(None)
        } else {
            let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
            let Some(entry) = self.transitions.get_mut(index) else {
                return Err(index_oob());
            };
            self.index = index;
            self.skips = entry.skips();
            self.past_len -= 1;
            Ok(Some(&mut entry.value))
        }
    }
    pub fn forward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if let Some(entry) = self.transitions.get_mut(self.index) {
            self.past_len += 1;
            if self.skips < entry.skips() {
                self.skips += 1;
                Ok(None)
            } else {
                self.index += 1;
                self.skips = 0;
                Ok(Some(&mut entry.value))
            }
        } else if self.skips < self.skips_max {
            self.past_len += 1;
            self.skips += 1;
            Ok(None)
        } else {
            Err(OutOfLog)
        }
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        if self.index == 0 {
            // if the current log position is at the past end, transitions.front() is not a past value but a future value
            return None;
        }
        let excessive_len = self.past_len.checked_sub(max_past_len)?;
        let past_end = self.transitions.front()?;
        if excessive_len >= past_end.len() {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> RareDrain<T> {
        let mut drain_amount = 0;
        for entry in self.transitions.iter() {
            let less = self.past_len - entry.len();
            if less < max_past_len {
                break;
            }
            self.past_len = less;
            drain_amount += 1;
        }
        self.index -= drain_amount;
        RareDrain(self.transitions.drain(..drain_amount))
    }
    fn pop_past(&mut self) -> Option<T> {
        self.transitions.pop_front().map(|rare| {
            self.index -= 1;
            self.past_len -= rare.len();
            rare.value
        })
    }
}

impl<T: LoggedAt> RareTransitionLog<T> {
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<T> {
        if self.index == 0 {
            // if the current log position is at the past end, transitions.front() is not a past value but a future value
            return None;
        }
        let logged_at = self.transitions.front()?.logged_at();
        if !meta.contains_in_past(logged_at, false, true) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> RareDrain<T> {
        // may be redundant but if not improves partition_point performance
        self.transitions.truncate(self.index);

        let past_len = meta.past_world_states();
        let to = self
            .transitions
            .partition_point(|entry| meta.frames_before_present(entry.logged_at()) >= past_len);
        self.past_len -= to // `to` plus sum of `RareValue::skips` == sum of `RareValue::len` but with less operations 
            + self
                .transitions
                .range(..to)
                .map(RareValue::skips)
                .sum::<usize>();
        self.index -= to;
        RareDrain(self.transitions.drain(..to))
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU32;

    use serde::{Deserialize, Serialize};

    use crate::{
        log::test::{shorten_strategy, ShortenStrategy},
        RevFrame,
    };

    use super::*;

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: RareTransitionLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: RareTransitionLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: RareTransitionLog<char>,
        }

        let mut original = RareTransitionLog::new();
        original.push_present(Some('a'));
        original.push_present(Some('b'));
        original.backward_log().expect("in log");

        let mut logs = Logs {
            full: original.clone(),
            full_with_capacity: original.clone(),
            logless_with_capacity: original.clone(),
        };

        logs.full.transitions_reserve_exact(98);
        logs.full_with_capacity.transitions_reserve_exact(98);
        logs.logless_with_capacity.transitions_reserve_exact(98);

        let serialized = serde_json::to_string_pretty(&logs).unwrap();
        let Logs {
            full,
            full_with_capacity,
            logless_with_capacity,
        } = serde_json::from_str(&serialized).unwrap();

        let test = |log: &RareTransitionLog<char>, len, with_capacity| {
            assert_eq!(
                log.transitions_len(),
                len,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}"
            );
            assert_eq!(
                log.transitions_capacity() >= 100,
                with_capacity,
                "before: {original:#?}\nserialized: {serialized}\nafter: {log:#?}\ncapacity: {}",
                log.transitions_capacity()
            );
        };

        test(&full, 2, false);
        test(&full_with_capacity, 2, true);
        test(&logless_with_capacity, 0, true);
    }

    impl RareTransitionLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            push: Option<u8>,
            expected_transitions_len: usize,
            expected_popped: Option<(u8, u32)>,
        ) {
            meta.queue_forward();
            meta.update(|_, _| {});
            let before = self.clone();
            let push = push.map(|transition| (transition, meta.present_world_state()));
            self.push_present(push);
            let after_push = self.clone();
            let actual_popped = shorten_strategy!(
                self,
                meta,
                strategy,
                meta.past_world_states(),
                before,
                after_push
            );
            assert_eq!(
                actual_popped, expected_popped,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.transitions_len(),
                expected_transitions_len,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transition: Result<Option<u8>, OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transition = expected_transition.map(|transition| {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
                transition.map(|transition| (transition, frame))
            });
            let actual_transition = self.forward_log().map(|transition| transition.cloned());
            assert_eq!(
                actual_transition, expected_transition,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            )
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_transition: Result<Option<u8>, OutOfLog>,
        ) {
            let before = self.clone();
            let expected_transition = expected_transition.map(|transition| {
                let frame = meta.present_world_state();
                meta.queue_log(frame.wrapping_sub(1)).unwrap();
                meta.update(|_, _| {});
                transition.map(|transition| (transition, frame))
            });
            let actual_transition = self.backward_log().map(|transition| transition.cloned());
            assert_eq!(
                actual_transition, expected_transition,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            )
        }
        fn test_drain_future(
            &self,
            expected_future: impl IntoIterator<Item = (u8, u32)>,
            expected_transitions_len: usize,
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
                clone.transitions_len(),
                expected_transitions_len,
                "\nbefore: {before:#?}\nafter: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut log = RareTransitionLog::new();

            log.test_forward(meta, strategy, Some(1), 1, None);
            log.test_forward(meta, strategy, None, 1, None);
            // shortened log
            log.test_forward(meta, strategy, Some(3), 1, Some((1, 1)));

            log.test_backward_log(meta, Ok(Some(3)));
            log.test_backward_log(meta, Ok(None));
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, Err(OutOfLog));

            log.test_forward_log(meta, Ok(None));
            log.test_forward_log(meta, Ok(Some(3)));
            // out of log, no mutations happend to both meta and log here
            log.test_forward_log(meta, Err(OutOfLog));

            log.test_backward_log(meta, Ok(Some(3)));
            log.test_backward_log(meta, Ok(None));

            let clone = log.test_drain_future([(3, 3)], 0);

            for mut log in [log, clone] {
                // all entries are truncated as they are in the future
                log.test_forward(meta, strategy, None, 0, None);
            }
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareTransitionLog<RevFrame>>();
    }
}
