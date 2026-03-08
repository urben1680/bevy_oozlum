use core::mem::Discriminant;

use super::*;

impl RevDirection {
    const FWD_DISCRIMINANT: Discriminant<Self> = Self::FORWARD_MIN.discriminant();
    const fn discriminant(self) -> Discriminant<Self> {
        core::mem::discriminant(&self)
    }
}

struct RunValues {
    past_end: u64,
    now: u64,
    log_exits: u64,
    log_clears: u64,
    direction: Discriminant<RevDirection>,
}

impl RevMeta {
    fn update_assert(&mut self, queue: Option<RevQueue>, values: Option<RunValues>) {
        match queue {
            None if self.now == 0 => assert_eq!(self.get_queue(), Some(RevQueue::RunForward)),
            None => assert_eq!(self.get_queue(), None),
            Some(queue) => self.set_queue(queue),
        }
        self.update_ref(Ok(values.is_some()), |meta, direction| {
            let values = values.unwrap();
            assert_eq!(meta.past_end(), values.past_end);
            assert_eq!(meta.now(), values.now);
            assert_eq!(meta.log_exits(), values.log_exits);
            assert_eq!(meta.log_clears(), values.log_clears);
            assert_eq!(direction.discriminant(), values.direction);
        });
    }
}

#[test]
fn traverses_log() {
    let mut meta = RevMeta::new(NonZeroU64::new(4).unwrap(), false);
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 0,
            now: 1,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunForward),
        Some(RunValues {
            past_end: 0,
            now: 2,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 0,
            now: 3,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 0,
            now: 4,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 1,
            now: 4,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now: 3,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 1,
            now: 2,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now: 1,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(None, None);
    meta.update_assert(Some(RevQueue::RunBackwardLog), None);
    meta.update_assert(
        Some(RevQueue::RunForwardLog),
        Some(RunValues {
            past_end: 1,
            now: 2,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::ForwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now: 3,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::ForwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunForwardLog),
        Some(RunValues {
            past_end: 1,
            now: 4,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::ForwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::ForwardLog.discriminant(),
        }),
    );
    meta.update_assert(None, None);
    meta.update_assert(Some(RevQueue::RunForwardLog), None);
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 1,
            now: 4,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now: 3,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(Some(RevQueue::Pause), None);
    meta.update_assert(
        Some(RevQueue::RunForward),
        Some(RunValues {
            past_end: 1,
            now: 4,
            log_exits: 1,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 1,
            now: 3,
            log_exits: 1,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now: 2,
            log_exits: 1,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(Some(RevQueue::ClearThenPause), None);
    meta.update_assert(
        Some(RevQueue::RunForward),
        Some(RunValues {
            past_end: 2,
            now: 3,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 2,
            now: 4,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 2,
            now: 5,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 2,
            now: 4,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 2,
            now: 3,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        Some(RevQueue::ClearThenRunForward),
        Some(RunValues {
            past_end: 3,
            now: 4,
            log_exits: 0,
            log_clears: 2,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
}
