use core::fmt::Debug;
use std::collections::{TryReserveError, VecDeque};

use bevy::reflect::Reflect;

use crate::meta::RevMeta;

use super::index_oob;

// todo: mention limitations, like missing frames
#[derive(Debug, Clone, Default, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FrameTransitionLog {
    frames: VecDeque<u64>,
    index: usize,
}

#[cfg(feature = "serde")]
mod serde_with {
    use std::collections::VecDeque;

    use crate::log::serde_with::{LoglessWithCapacity, WithCapacity, WithCapacityWrapper};

    use super::FrameTransitionLog;

    impl WithCapacity for FrameTransitionLog {
        type Se<'se> = (WithCapacityWrapper<&'se VecDeque<u64>>, usize);
        type De = (WithCapacityWrapper<VecDeque<u64>>, usize);
        fn get_with_capacity(&self) -> Self::Se<'_> {
            (WithCapacityWrapper(&self.frames), self.index)
        }
        fn from_with_capacity((WithCapacityWrapper(frames), index): Self::De) -> Self {
            Self { frames, index }
        }
    }

    impl LoglessWithCapacity for FrameTransitionLog {
        type Se<'se> = usize;
        type De = usize;
        fn get_logless_with_capacity(&self) -> Self::Se<'_> {
            self.frames.capacity()
        }
        fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Self {
            Self::with_capacity(logless_with_capacity)
        }
    }
}

impl FrameTransitionLog {
    pub const fn new() -> Self {
        Self {
            frames: VecDeque::new(),
            index: 0,
        }
    }
    pub fn with_capacity(frame_capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(frame_capacity),
            index: 0,
        }
    }
    pub fn frames_len(&self) -> usize {
        self.frames.len()
    }
    pub fn frames_capacity(&self) -> usize {
        self.frames.capacity()
    }
    pub fn frames_is_empty(&self) -> bool {
        self.frames.is_empty()
    }
    pub fn frames_reserve(&mut self, additional: usize) {
        self.frames.reserve(additional)
    }
    pub fn frames_reserve_exact(&mut self, additional: usize) {
        self.frames.reserve_exact(additional)
    }
    pub fn frames_try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.frames.try_reserve(additional)
    }
    pub fn frames_try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.frames.try_reserve_exact(additional)
    }
    pub fn frames_shrink_to(&mut self, min_capacity: usize) {
        self.frames.shrink_to(min_capacity)
    }
    pub fn frames_shrink_to_fit(&mut self) {
        self.frames.shrink_to_fit()
    }
    pub fn push_and_get_past_len(&mut self, meta: &RevMeta) -> usize {
        self.frames.truncate(self.index);
        let to_drain = self
            .frames
            .partition_point(|frame| *frame < meta.past_end());
        self.frames.drain(..to_drain);
        self.frames.push_back(meta.now());
        self.index = self.index + 1 - to_drain;
        self.index
    }
    pub fn clear(&mut self) {
        self.frames.clear();
        self.index = 0;
    }
    pub fn backward_log(&mut self, meta: &RevMeta) -> bool {
        let Some(index) = self.index.checked_sub(1) else {
            return false;
        };
        let Some(&frame) = self.frames.get(index) else {
            index_oob();
            return false;
        };
        let expects_backward = frame == meta.now() + 1;
        if expects_backward {
            self.index = index;
        }
        expects_backward
    }
    pub fn forward_log(&mut self, meta: &RevMeta) -> bool {
        let Some(&frame) = self.frames.get(self.index) else {
            return false;
        };
        let expects_forward = frame == meta.now();
        if expects_forward {
            self.index += 1;
        }
        expects_forward
    }
}

#[cfg(test)]
mod test {
    use std::num::NonZeroU64;

    use super::*;

    struct Log {
        log: FrameTransitionLog,
        meta: RevMeta,
    }

    impl Log {
        fn new(max_world_states: u64, now: u64) -> Self {
            let log = FrameTransitionLog::new();
            let meta = RevMeta::new(NonZeroU64::new(max_world_states), now, false);
            Self { log, meta }
        }
        fn forward(&mut self, updates_with_expected_past_len: Vec<usize>) {
            self.meta.queue_not_log_forward();
            self.meta
                .update(|meta| {
                    let before = self.log.clone();
                    let len = updates_with_expected_past_len.len();
                    let updates_with_actual_past_len: Vec<usize> = (0..len)
                        .map(|_| self.log.push_and_get_past_len(meta))
                        .collect();
                    assert_eq!(
                        updates_with_actual_past_len, updates_with_expected_past_len,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    )
                })
                .expect("should update");
        }
        fn forward_log(&mut self, expected_forward_log_updates: usize) {
            let previous = self.meta.now() + 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| {
                    for _ in 0..expected_forward_log_updates {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.forward_log(meta),
                            true,
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                            self.log
                        );
                    }
                    let before = self.log.clone();
                    assert_eq!(
                        self.log.forward_log(meta),
                        false,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    );
                })
                .expect("should update");
        }
        fn backward_log(&mut self, expected_backward_log_updates: usize) {
            let previous = self.meta.now() - 1;
            assert_eq!(self.meta.queue_log(previous), Ok(1));
            self.meta
                .update(|meta| {
                    for _ in 0..expected_backward_log_updates {
                        let before = self.log.clone();
                        assert_eq!(
                            self.log.backward_log(meta),
                            true,
                            "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                            self.log
                        );
                    }
                    let before = self.log.clone();
                    assert_eq!(
                        self.log.backward_log(meta),
                        false,
                        "\nbefore: {before:#?}\nafter: {:#?}\nmeta: {meta:?}",
                        self.log
                    );
                })
                .expect("should update");
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut log = Log::new(4, 0);
        log.forward(vec![1]); // frame #1
        log.forward(vec![2, 3]); // frame #2
        log.forward(vec![4]);
        log.forward(vec![]);
        // shortened log
        log.forward(vec![4, 5]);

        log.backward_log(2);
        log.backward_log(0);
        log.backward_log(1);

        log.forward_log(1);
        log.forward_log(0);
        log.forward_log(2);

        log.backward_log(2);
        log.backward_log(0);
        log.backward_log(1);

        log.forward(vec![3]);
    }
}
