use core::fmt::Debug;
use std::collections::{
    vec_deque::Drain,
    TryReserveError,
};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::DenseStateLog;

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrameStateLog(DenseStateLog<u64>);

#[cfg(feature = "serde")]
mod serde_with {
    use crate::log::serde_with::{
        LoglessState, LoglessWithCapacity, WithCapacity,
    };

    use super::{FrameStateLog, DenseStateLog};

    impl LoglessState for FrameStateLog {
        type Se<'se> = <DenseStateLog<u64> as LoglessState>::Se<'se>;
        type De = <DenseStateLog<u64> as LoglessState>::De;
        fn get_logless_state(&self) -> Self::Se<'_> {
            self.0.get_logless_state()
        }
        fn from_logless_state(logless_state: Self::De) -> Self {
            Self(DenseStateLog::<u64>::from_logless_state(logless_state))
        }
    }

    impl WithCapacity for FrameStateLog {
        type Se<'se> = <DenseStateLog<u64> as WithCapacity>::Se<'se>;
        type De = <DenseStateLog<u64> as WithCapacity>::De;
        fn get_with_capacity(&self) -> Self::Se<'_> {
            self.0.get_with_capacity()
        }
        fn from_with_capacity(with_capacity: Self::De) -> Self {
            Self(DenseStateLog::<u64>::from_with_capacity(with_capacity))
        }
    }

    impl LoglessWithCapacity for FrameStateLog {
        type Se<'se> = <DenseStateLog<u64> as LoglessWithCapacity>::Se<'se>;
        type De = <DenseStateLog<u64> as LoglessWithCapacity>::De;
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            self.0.get_logless_with_capacity()
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self(DenseStateLog::<u64>::from_logless_with_capacity(logless_with_capacity))
        }
    }
}

impl FrameStateLog {
    pub const fn new(meta: &RevMeta) -> Self {
        Self(DenseStateLog::new(meta.present_world_state()))
    }
    pub fn with_capacity(meta: &RevMeta, frames_capacity: usize) -> Self {
        Self(DenseStateLog::with_capacity(meta.present_world_state(), frames_capacity))
    }
    pub fn past_len(&self) -> usize {
        self.0.past_len()
    }
    pub fn frames_len(&self) -> usize {
        self.0.states_len()
    }
    pub fn frames_capacity(&self) -> usize {
        self.0.states_capacity()
    }
    pub fn frames_is_empty(&self) -> bool {
        self.0.states_is_empty()
    }
    pub fn frames_reserve(&mut self, additional: usize) {
        self.0.states_reserve(additional)
    }
    pub fn frames_reserve_exact(&mut self, additional: usize) {
        self.0.states_reserve_exact(additional)
    }
    pub fn frames_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.0.states_try_reserve(additional)
    }
    pub fn frames_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.0.states_try_reserve_exact(additional)
    }
    pub fn frames_shrink_to(&mut self, min_capacity: usize) {
        self.0.states_shrink_to(min_capacity)
    }
    pub fn frames_shrink_to_fit(&mut self) {
        self.0.states_shrink_to_fit()
    }
    pub fn push_and_drain_past(&mut self, meta: &RevMeta) -> Drain<u64> {
        self.0.frame_push_and_drain_past(meta)
    }
    pub fn drain_future(&mut self) -> Drain<u64> {
        self.0.drain_future()
    }
    pub fn clear(&mut self) {
        self.0.clear()
    }
    pub fn clear_with(&mut self, meta: &RevMeta) {
        self.0.clear_with(meta.present_world_state())
    }
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        self.0.frame_backward_log(meta)
    }
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        self.0.frame_forward_log(meta)
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU64;

    use super::*;

    struct Log {
        log: FrameStateLog,
        meta: RevMeta
    }

    impl Log {
        fn new(max_world_states: u64, now: u64) -> Self {
            let meta = RevMeta::new(NonZeroU64::new(max_world_states), Some(now), false);
            let log = FrameStateLog::new(&meta);
            Self {
                log,
                meta
            }
        }
        fn forward_skip(
            &mut self
        ) {
            self.meta.queue_forward();
            self.meta.update(|_|());
        }
        fn forward(
            &mut self,
            push: u64,
            expected_past_len: usize,
            expected_drain: Vec<u64>,
        ) {
            self.meta.queue_forward();
            self.meta.update(|meta| {
                assert_eq!(meta.present_world_state(), push, "\n{meta:?}");
                let expected_drain: Vec<_> = expected_drain.into_iter().collect();
                let before = self.log.clone();
                let actual_drain: Vec<_> = self.log.push_and_drain_past(meta).collect();
                assert_eq!(
                    &actual_drain,
                    &expected_drain,
                    "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}", self.log
                );
                assert_eq!(
                    self.log.past_len(),
                    expected_past_len,
                    "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}", self.log
                );
            });
        }
        fn forward_log(&mut self, result: bool) {
            let previous = self.meta.present_world_state() + 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta.update(|meta| {
                let before = self.log.clone();
                assert_eq!(
                    self.log.forward_log(meta), 
                    result,
                    "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}", self.log
                );
            });
        }
        fn backward_log(&mut self, result: bool) {
            let previous = self.meta.present_world_state() - 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta.update(|meta| {
                let before = self.log.clone();
                assert_eq!(
                    self.log.backward_log(meta), 
                    result,
                    "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}", self.log
                );
            });
        }
    }

    #[test] // todo: test multiple times of pushes per frame: `times` param
    fn log_traversal_works() {
        let mut log = Log::new(4, 0);
        log.forward(1, 1, vec![]);
        log.forward(2, 2, vec![]);
        log.forward_skip();
        // shortened log
        log.forward(4, 2, vec![0]);
        log.forward_skip();

        log.backward_log(false);
        log.backward_log(true);
        log.backward_log(false);

        log.forward_log(false);
        log.forward_log(true);
        log.forward_log(false);
    }
}
