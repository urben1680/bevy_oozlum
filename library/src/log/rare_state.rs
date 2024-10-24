use core::fmt::Debug;
use std::{
    collections::{TryReserveError, VecDeque},
    ops::Deref,
};

use bevy::{reflect::Reflect, utils::tracing::error};

use crate::{meta::RevMeta, RevFrame};

use super::{LogIter, LoggedAt, OutOfLog, PackedRevFrame, RareValue, INDEX_OOB};

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareStateLog<T> {
    /// RareValue.skips represents the number of None pushes after the state in the struct
    states: VecDeque<RareValue<T>>,
    present: RareValue<T>,
    index: usize,
    skips: usize,
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
            &self.present.value
        }
        fn from_logless_state(logless_state: Self::De) -> Result<Self, String> {
            Ok(logless_state.into())
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> WithCapacity for RareStateLog<T> {
        type Se<'se> = (
            WithCapacityWrapper<&'se VecDeque<RareValue<T>>>,
            &'se RareValue<T>,
            usize,
            usize,
            usize,
        );
        type De = (
            WithCapacityWrapper<VecDeque<RareValue<T>>>,
            RareValue<T>,
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
                self.past_len,
            )
        }
        fn from_with_capacity(
            (WithCapacityWrapper(states), present, index, skips, len): Self::De,
        ) -> Result<Self, String> {
            Ok(Self {
                states,
                present,
                index,
                skips,
                past_len: len,
            })
        }
    }

    impl<T: Serialize + for<'de> Deserialize<'de> + 'static> LoglessWithCapacity for RareStateLog<T> {
        type Se<'se> = (&'se T, usize);
        type De = (T, usize);
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            (&self.present.value, self.states_capacity())
        }
        fn from_logless_with_capacity(
            (present, states_capacity): Self::De,
        ) -> Result<Self, String> {
            Ok(Self::with_capacity(present, states_capacity))
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
        &self.present.value
    }
}

impl<T> RareStateLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            states: VecDeque::new(),
            present: RareValue {
                value: present,
                skips: PackedRevFrame::MIN,
            },
            index: 0,
            skips: 0,
            past_len: 0,
        }
    }

    pub fn with_capacity(present: T, states_capacity: usize) -> Self {
        Self {
            states: VecDeque::with_capacity(states_capacity),
            present: RareValue {
                value: present,
                skips: PackedRevFrame::MIN,
            },
            index: 0,
            skips: 0,
            past_len: 0,
        }
    }
    pub fn into_inner(self) -> T {
        self.present.value
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
    fn past_end_rare(&self) -> Option<&RareValue<T>> {
        self.states.front()
    }
    /// Most past state or `None` if the oldest state is considered to be the present state
    pub fn past_end(&self) -> Option<&T> {
        if self.index == 0 {
            return None;
        }
        self.past_end_rare().map(|rare| &rare.value)
    }
    pub fn pop_past(&mut self) -> Option<T> {
        if self.index == 0 {
            return None;
        }
        self.states.pop_front().map(|rare| {
            self.index -= 1;
            self.past_len -= rare.len();
            rare.value
        })
    }
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.states.drain(self.index..).map(|rare| rare.value)
    }
    pub fn clear(&mut self) {
        self.states.clear();
        self.present.skips = PackedRevFrame::MIN;
        self.index = 0;
        self.past_len = 0;
        self.skips = 0;
    }
    pub fn clear_with(&mut self, present: T) {
        self.states.clear();
        self.present = RareValue {
            value: present,
            skips: PackedRevFrame::MIN,
        };
        self.index = 0;
        self.past_len = 0;
        self.skips = 0;
    }
    pub fn push_present(&mut self, state: Option<T>) {
        self.states.truncate(self.index);
        self.past_len += 1;
        match state {
            None if self.skips < PackedRevFrame::MAX_AS_USIZE => {
                self.skips += 1;
                self.present.skips = RevFrame::new(self.skips).into();
            }
            Some(state) => {
                self.present.skips = RevFrame::new(self.skips).into();
                let previous = core::mem::replace(
                    &mut self.present,
                    RareValue {
                        value: state,
                        skips: PackedRevFrame::MIN,
                    },
                );
                self.states.push_back(previous);
                self.skips = 0;
                self.index += 1;
            }
            None => {}
        }
    }
    pub fn backward_log(&mut self) -> Result<(), OutOfLog> {
        if self.skips > 0 {
            self.skips -= 1;
            self.past_len -= 1;
            return Ok(());
        }
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        if let Some(entry) = self.states.get_mut(index) {
            self.index = index;
            core::mem::swap(&mut self.present, entry);
            self.skips = self.present.skips.into();
            self.past_len -= 1;
            return Ok(());
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct RareStateLogDebug {
            states_len: usize,
            present_skips: usize,
            index: usize,
            skips: usize,
            len: usize,
        }

        let debug_struct = RareStateLogDebug {
            states_len: self.states.len(),
            present_skips: self.present.skips.into(),
            index: self.index,
            skips: self.skips,
            len: self.past_len,
        };

        error!("{INDEX_OOB}, {debug_struct:#?}");
        Err(OutOfLog)
    }
    pub fn forward_log(&mut self) -> Result<(), OutOfLog> {
        if self.skips < self.present.skips.into() {
            self.past_len += 1;
            self.skips += 1;
            Ok(())
        } else if let Some(entry) = self.states.get_mut(self.index) {
            self.past_len += 1;
            self.index += 1;
            self.skips = 0;
            core::mem::swap(&mut self.present, entry);
            Ok(())
        } else {
            Err(OutOfLog)
        }
    }
    pub fn pop_past_by_len(&mut self, max_past_len: usize) -> Option<T> {
        let excessive_len = self.past_len.checked_sub(max_past_len)?;
        let past_end = self.past_end_rare()?;
        if excessive_len >= past_end.len() {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
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
        self.states.drain(..drain_amount).map(|rare| rare.value)
    }
}

impl<T: LoggedAt> RareStateLog<T> {
    pub fn pop_past_by_logged_at(&mut self, meta: &RevMeta) -> Option<T> {
        let entry = self.past_end_rare()?;
        let logged_at = entry.value.logged_at().wrapping_add(entry.skips());
        if !meta.past_contains(logged_at) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> impl LogIter<T> {
        // may be redundant but if not improves partition_point performance
        self.states.truncate(self.index);

        let ref_len = meta.past_world_states();
        let start = meta.oldest_world_state();
        let to = self
            .states
            .partition_point(|entry| !RevMeta::contains_buffered(start, entry, ref_len));
        self.past_len -= to // sum of to-be-drained states, because of this mapping RareValue::len below is not needed, only skips_before_state
            + self
                .states
                .range(..to)
                .map(RareValue::skips)
                .sum::<usize>();
        self.index -= to;
        self.states.drain(..to).map(|rare| rare.value)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use serde::{Deserialize, Serialize};

    use crate::log::test::{shorten_strategy, ShortenStrategy};

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

    impl RareStateLog<(u8, RevFrame)> {
        fn test_forward(
            &mut self,
            meta: &mut RevMeta,
            strategy: ShortenStrategy,
            state: u8,
            state_is_pushed: bool,
            expected_states_len: usize,
            expected_popped: Option<(u8, usize)>,
        ) {
            meta.queue_forward();
            meta.update();
            let before = self.clone();
            let push = state_is_pushed.then_some((state, meta.present_world_state()));
            self.push_present(push);
            let after_push = self.clone();
            let actual_popped = shorten_strategy!(self, meta, strategy, before, after_push);
            assert_eq!(
                actual_popped, expected_popped,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            assert_eq!(
                self.states_len(),
                expected_states_len,
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter_push: {after_push:#?}\nafter_pop: {self:#?}",
            );
            self.test_state(before, meta, state);
        }
        fn test_forward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: u8,
            possibly_out_of_log: bool,
        ) {
            if possibly_out_of_log {
                // Depending on skips before the current state, no OutOfLog occurs here.
                // Instead, it is only asserted that the method call does not panic.
                let _ = self.clone().forward_log();
                return;
            }
            let before = self.clone();
            let frame = meta.present_world_state().wrapping_add(1);
            meta.queue_log(frame).unwrap();
            meta.update();
            let result = self.forward_log();
            assert_eq!(
                result,
                Ok(()),
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
            self.test_state(before, meta, expected_state);
        }
        fn test_backward_log(
            &mut self,
            meta: &mut RevMeta,
            expected_state: u8,
            possibly_out_of_log: bool,
        ) {
            if possibly_out_of_log {
                // Depending on skips before the current state, no OutOfLog occurs here.
                // Instead, it is only asserted that the method call does not panic.
                let _ = self.clone().backward_log();
                return;
            }
            let before = self.clone();
            let frame = meta.present_world_state().wrapping_sub(1);
            meta.queue_log(frame).unwrap();
            meta.update();
            let result = self.backward_log();
            assert_eq!(
                result,
                Ok(()),
                "\nmeta: {meta:#?}\nbefore: {before:#?}\nafter: {self:#?}",
            );
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
            expected_future: impl IntoIterator<Item = (u8, usize)>,
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
            clone
        }
    }

    #[test]
    fn push_and_log_traversal() {
        for strategy in ShortenStrategy::VARIANTS {
            let meta = &mut RevMeta::new(NonZeroUsize::new(3), 0, false);
            let mut log = RareStateLog::new((0, meta.present_world_state()));
        }
    }

    /*
    #[test]
    fn test() {
        let mut meta_and_logs = MetaAndLogs::new(0, NonZeroUsize::new(3));

        // minimum_log_len remains < max_len because the current state is not considered to be part of the log
        meta_and_logs.forward(0, false, 0, 0);
        meta_and_logs.forward(0, false, 1, 0);
        meta_and_logs.forward(0, false, 2, 0);
        meta_and_logs.forward(0, false, 2, 0);

        // states_len is reduced by max_len
        meta_and_logs.forward(1, true, 2, 1);
        meta_and_logs.forward(1, false, 2, 1);
        meta_and_logs.forward(1, false, 2, 0);

        meta_and_logs.forward(2, true, 2, 1);
        meta_and_logs.forward(2, false, 2, 1);

        meta_and_logs.backward_log(Ok(2));
        meta_and_logs.backward_log(Ok(1));
        //meta_and_logs.backward_log(Err(OutOfLog)); // todo:
        // - panics because T from before log start is still in the log because the entry's skips is needed
        // - the log cannot determine the log end
        // - this is not an issue for usage as pop_front_by_... does not guarantee a minimal log len, just minimal states len

        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Ok(2));
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(2));
        meta_and_logs.backward_log(Ok(1));
        meta_and_logs.forward(1, false, 2, 0);
    }
    */

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareStateLog<PackedRevFrame>>();
    }
}
