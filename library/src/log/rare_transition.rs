use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::{
    reflect::{std_traits::ReflectDefault, Reflect},
    utils::tracing::error,
};

use crate::{meta::RevMeta, RevFrame};

use super::{LogIter, LoggedAt, OutOfLog, PackedRevFrame, RareValue, INDEX_OOB};

#[derive(Debug, Clone, Reflect)]
#[reflect(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RareTransitionLog<T> {
    /// RareValue.skips represents the number of None pushes before the transition in the struct
    transitions: VecDeque<RareValue<T>>,
    index: usize,
    skips: usize,
    /// Used to check for OutOfLog error when calling `self.forward_log`
    skips_max: usize,
    past_len: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use serde::{Deserialize, Serialize};

    use crate::{log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper}, RevFrame};

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
            RevFrame, // deserializes from usize and asserts value in range
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
            (WithCapacityWrapper(transitions), index, skips, RevFrame(skips_max), past_len): Self::De,
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
    pub fn drain_future(&mut self) -> impl LogIter<T> {
        self.skips_max = self.skips;
        self.transitions.drain(self.index..).map(|rare| rare.value)
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
            None if self.skips < PackedRevFrame::MAX_AS_USIZE => {
                self.skips += 1;
                self.past_len += 1;
            }
            Some(transition) => {
                self.transitions.push_back(RareValue {
                    value: transition,
                    skips: RevFrame::new(self.skips).into(),
                });
                self.index += 1;
                self.skips = 0;
                self.past_len += 1;
            }
            None => {} // assume user will not go back before current entry
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
            let transitions_len = self.transitions.len();
            if let Some(entry) = self.transitions.get_mut(index) {
                self.index = index;
                self.skips = entry.skips.into();
                self.past_len -= 1;
                return Ok(Some(&mut entry.value));
            }

            #[derive(Debug)]
            #[allow(dead_code)]
            struct RareTransitionLogDebug {
                transitions_len: usize,
                index: usize,
                skips: usize,
                skips_max: usize,
                len: usize,
            }

            let debug_struct = RareTransitionLogDebug {
                transitions_len,
                index: self.index,
                skips: self.skips,
                skips_max: self.skips_max,
                len: self.past_len,
            };

            error!("{INDEX_OOB}, {debug_struct:#?}");
            Err(OutOfLog)
        }
    }
    pub fn forward_log(&mut self) -> Result<Option<&mut T>, OutOfLog> {
        if let Some(entry) = self.transitions.get_mut(self.index) {
            self.past_len += 1;
            if self.skips < entry.skips.into() {
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
    pub fn drain_past_by_len(&mut self, max_past_len: usize) -> impl LogIter<T> {
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
        self.transitions
            .drain(..drain_amount)
            .map(|rare| rare.value)
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
        if !meta.past_exclusive_oldest_contains(logged_at) {
            self.pop_past()
        } else {
            None
        }
    }
    pub fn truncate_future_drain_past_by_logged_at(&mut self, meta: &RevMeta) -> impl LogIter<T> {
        // may be redundant but if not improves partition_point performance
        self.transitions.truncate(self.index);
        self.skips_max = self.skips;

        let ref_len = meta.past_world_states();
        let to = self
            .transitions
            .partition_point(|entry| meta.before_past_buffered(entry, ref_len));
        self.past_len -= to // sum of to-be-drained transitions, because of this mapping RareValue::len below is not needed, only skips_before_value
            + self
                .transitions
                .range(..to)
                .map(RareValue::skips)
                .sum::<usize>();
        self.index -= to;
        self.transitions.drain(..to).map(|rare| rare.value)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroUsize;

    use serde::{Deserialize, Serialize};

    use crate::log::PackedRevFrame;

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

    /*
    #[test]
    fn test() {
        let mut meta_and_logs = MetaAndLogs::new(NonZeroUsize::new(3));

        meta_and_logs.forward(None, 1, 0);
        meta_and_logs.forward(Some(1), 2, 1);
        // pop_front called internally
        meta_and_logs.forward(Some(2), 2, 2);
        meta_and_logs.forward(None, 2, 1);

        meta_and_logs.backward_log(Ok(None));
        meta_and_logs.backward_log(Ok(Some(2)));
        // out of log, no mutations happend to both meta and log here
        meta_and_logs.backward_log(Err(OutOfLog));

        meta_and_logs.forward_log(Ok(Some(2)));
        meta_and_logs.forward_log(Ok(None));
        // nothing ever logged past 8, no mutations happend to both meta and log here
        // todo: would this test fail if no value was pushed at the second forward? same situation as RareValueLog
        meta_and_logs.forward_log(Err(OutOfLog));

        meta_and_logs.backward_log(Ok(None));
        meta_and_logs.backward_log(Ok(Some(2)));
        // all entries are truncated as they are in the future, the new logged entry increases len to 1
        meta_and_logs.forward(Some(3), 1, 1);
    }
    */

    #[allow(dead_code)]
    fn impls_reflect() {
        bevy::reflect::TypeRegistry::empty().register::<RareTransitionLog<PackedRevFrame>>();
    }
}
