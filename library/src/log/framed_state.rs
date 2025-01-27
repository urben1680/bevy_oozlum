use core::fmt::Debug;
use std::{
    collections::{
        vec_deque::{Drain, Iter},
        TryReserveError, VecDeque,
    },
    ops::Deref,
};

use bevy::reflect::Reflect;

use crate::{log::index_oob, meta::RevMeta};

use super::{FramedDrain, FramedMeta, OutOfLog, ValueLoggedAt};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FramedStateLog<T> {
    /// The log of states, with two partitions:
    /// - Past states in the indices `[0, self.index[`
    /// - Future states in the indices `[self.index, self.states.len()[`
    ///
    /// The present state is not part of this deque and traversing the log swaps
    /// the present state from before and now while keeping the above partitions.
    states: VecDeque<ValueLoggedAt<T>>,
    /// The present state, easily accessible to read.
    present: ValueLoggedAt<T>,
    /// The index of the nearest future state in `self.states`, if there is any.
    ///
    /// Never larger than `self.states.len()`
    index: usize,
    framed: FramedMeta,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::log::serde_with::{
        LoglessState, LoglessWithCapacity, WithCapacity, WithCapacityWrapper,
    };

    use super::{FramedMeta, FramedStateLog, ValueLoggedAt};

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessState for FramedStateLog<T> {
        type Se<'se> = (&'se ValueLoggedAt<T>, FramedMeta);
        type De = (ValueLoggedAt<T>, FramedMeta);
        fn get_logless_state(&self) -> Self::Se<'_> {
            (&self.present, self.framed)
        }
        fn from_logless_state((present, framed): Self::De) -> Self {
            Self {
                states: VecDeque::new(),
                present,
                index: 0,
                framed,
            }
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for FramedStateLog<T> {
        type Se<'se> = (
            WithCapacityWrapper<&'se VecDeque<ValueLoggedAt<T>>>,
            &'se ValueLoggedAt<T>,
            usize,
            FramedMeta,
        );
        type De = (
            WithCapacityWrapper<VecDeque<ValueLoggedAt<T>>>,
            ValueLoggedAt<T>,
            usize,
            FramedMeta,
        );
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (
                WithCapacityWrapper(&self.states),
                &self.present,
                self.index,
                self.framed,
            )
        }
        fn from_with_capacity(
            (WithCapacityWrapper(states), present, index, framed): Self::De,
        ) -> Self {
            Self {
                states,
                present,
                index,
                framed,
            }
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessWithCapacity for FramedStateLog<T> {
        type Se<'se> = (&'se ValueLoggedAt<T>, usize, FramedMeta);
        type De = (ValueLoggedAt<T>, usize, FramedMeta);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (&self.present, self.states.capacity(), self.framed)
        }
        fn from_logless_with_capacity((present, capacity, framed): Self::De) -> Self {
            Self {
                states: VecDeque::with_capacity(capacity),
                present,
                index: 0,
                framed,
            }
        }
    }
}

impl<T> Deref for FramedStateLog<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.present.value
    }
}

impl<T> FramedStateLog<T> {
    pub fn new(meta: &RevMeta, present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present: ValueLoggedAt::new(meta, present),
            index: 0,
            framed: FramedMeta::new(meta),
        }
    }
    pub fn with_capacity(meta: &RevMeta, present: T, states_capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(states_capacity),
            present: ValueLoggedAt::new(meta, present),
            index: 0,
            framed: FramedMeta::new(meta),
        }
    }
    pub fn into_inner(self) -> ValueLoggedAt<T> {
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
    pub fn push_and_drain_past(&mut self, meta: &RevMeta, state: T) -> FramedDrain<T> {
        let to_drain = self.framed.push_and_len_to_drain_past(
            meta,
            &mut self.states,
            Some(&mut self.present),
            &mut self.index,
            state,
        );
        FramedDrain(self.states.drain(..to_drain))
    }
    pub(super) fn push_and_iter_to_drain_past(
        &mut self,
        meta: &RevMeta,
        state: T,
    ) -> Iter<ValueLoggedAt<T>> {
        let to_drain = self.framed.push_and_len_to_drain_past(
            meta,
            &mut self.states,
            Some(&mut self.present),
            &mut self.index,
            state,
        );
        self.states.range(..to_drain)
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> FramedDrain<T> {
        FramedDrain(self.states.drain(..to_drain))
    }
    pub fn drain_future(&mut self) -> FramedDrain<T> {
        FramedDrain(self.states.drain(self.index..))
    }
    pub fn clear(&mut self) {
        self.states.clear();
        self.index = 0;
    }
    pub fn clear_with(&mut self, meta: &RevMeta, present: T) {
        self.states.clear();
        self.present = ValueLoggedAt::new(meta, present);
        self.framed = FramedMeta::new(meta);
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        // before:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2
        // after:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1

        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        let now_future = self.states.get_mut(index).ok_or_else(index_oob)?;
        self.index = index;
        core::mem::swap(&mut self.present, now_future);
        self.framed.backward_log(self.present.logged_at);
        Ok(())
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        // before:
        //  states:  [1, 3, 4]
        //  present: 2
        //  index:   1
        // after:
        //  states:  [1, 2, 4]
        //  present: 3
        //  index:   2

        let now_future = self.states.get_mut(self.index).ok_or(OutOfLog)?;
        core::mem::swap(&mut self.present, now_future);
        self.framed.forward_log(self.present.logged_at);
        self.index += 1;
        return Ok(());
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU32;

    use serde::{Deserialize, Serialize};

    use super::*;

    use crate::{
        frame::RevFrame,
        log::test::{shorten_strategy, ShortenStrategy},
        meta::RevMeta,
    };

    #[test]
    fn serde_with() {
        #[derive(Serialize, Deserialize)]
        struct Logs {
            full: FramedStateLog<char>,
            #[serde(with = "crate::log::logless_state")]
            logless: FramedStateLog<char>,
            #[serde(with = "crate::log::with_capacity")]
            full_with_capacity: FramedStateLog<char>,
            #[serde(with = "crate::log::logless_with_capacity")]
            logless_with_capacity: FramedStateLog<char>,
        }

        let meta = &RevMeta::default();
        let mut original = FramedStateLog::new(meta, 'a');
        original.push_and_drain_past(meta, 'b');
        original.push_and_drain_past(meta, 'c');
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

        let test = |log: &FramedStateLog<char>, len, with_capacity| {
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
        let meta = &RevMeta::default();
        let mut original = FramedStateLog::new(meta, 1);
        original.push_and_drain_past(meta, 2);
        original.push_and_drain_past(meta, 3);
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
        log.clear_with(meta, 4);
        assert_eq!(*log, 4, "log: {log:#?}\noriginal: {original:#?}");
        assert_eq!(
            log.states_len(),
            0,
            "log: {log:#?}\noriginal: {original:#?}"
        );
    }
    /*
    impl FramedStateLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            push: u8,
            expected_states_len: usize,
            expected_pop: Option<(u8, u32)>,
        ) {
            meta.queue_forward();
            meta.update(|_, _| {});
            let before = self.clone();
            let push = (push, meta.present_world_state());
            self.push(push);
            let after_push = self.clone();
            let actual_pop = shorten_strategy!(
                self,
                meta,
                strategy,
                meta.past_world_states(),
                before,
                after_push
            );
            assert_eq!(
                actual_pop, expected_pop,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.states_len(),
                expected_states_len,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                **self, push,
                "\nstrategy: {strategy:#?}\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
        }
        fn test_forward_log(&mut self, meta: &mut RevMeta, expected_state: u8, out_of_log: bool) {
            let before = self.clone();
            if out_of_log {
                let result = self.forward_log();
                assert_eq!(
                    result,
                    Err(OutOfLog),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            } else {
                let frame = meta.present_world_state().wrapping_add(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
                let result = self.forward_log();
                assert_eq!(
                    result,
                    Ok(()),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            }
            self.test_state(before, meta, expected_state);
        }
        fn test_backward_log(&mut self, meta: &mut RevMeta, expected_state: u8, out_of_log: bool) {
            let before = self.clone();
            if out_of_log {
                let result = self.backward_log();
                assert_eq!(
                    result,
                    Err(OutOfLog),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            } else {
                let frame = meta.present_world_state().wrapping_sub(1);
                meta.queue_log(frame).unwrap();
                meta.update(|_, _| {});
                let result = self.backward_log();
                assert_eq!(
                    result,
                    Ok(()),
                    "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
                );
            }
            self.test_state(before, meta, expected_state);
        }
        fn test_state(&self, before: Self, meta: &RevMeta, state: u8) {
            assert_eq!(
                **self,
                (state, meta.present_world_state()),
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
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            assert_eq!(
                clone.states_len(),
                expected_states_len,
                "\nbefore: {before:#?}\nafter_drain_future: {clone:#?}"
            );
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroU32::new(3), None, false);
            let mut log = FramedStateLog::new((0, meta.present_world_state()));

            log.test_forward(meta, strategy, 1, 1, None);
            log.test_forward(meta, strategy, 2, 2, None);
            // shortened log
            log.test_forward(meta, strategy, 3, 2, Some((0, 0)));

            log.test_backward_log(meta, 2, false);
            log.test_backward_log(meta, 1, false);
            // out of log, no mutations happend to both meta and log here
            log.test_backward_log(meta, 1, true);

            log.test_forward_log(meta, 2, false);
            log.test_forward_log(meta, 3, false);
            // nothing ever logged past 3, no mutations happend to both meta and log here
            log.test_forward_log(meta, 3, true);

            log.test_backward_log(meta, 2, false);
            log.test_backward_log(meta, 1, false);

            let clone = log.test_drain_future([(2, 2), (3, 3)], 0);

            for mut log in [log, clone] {
                // all entries are truncated as they are in the future
                log.test_forward(meta, strategy, 4, 1, None);
            }
        }
    }

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<FramedStateLog<RevFrame>>();
    }
    */
}
