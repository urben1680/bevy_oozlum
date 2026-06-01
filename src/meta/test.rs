use core::mem::Discriminant;

use super::*;

impl RevDirection {
    const FWD_DISCRIMINANT: Discriminant<Self> = Self::NOT_LOG_MIN.discriminant();
    const fn discriminant(self) -> Discriminant<Self> {
        core::mem::discriminant(&self)
    }
}

struct RunValues {
    past_end: u64,
    now_before_running: u64,
    now: u64,
    now_after_running: u64,
    future_end: u64,
    log_exits: u64,
    log_clears: u64,
    direction: Discriminant<RevDirection>,
}

impl RevMeta {
    fn update_assert(&mut self, queue: Option<RevQueue>, values: Option<RunValues>) {
        self.update_ref(queue, values.is_some(), |meta, direction| {
            let values = values.unwrap();
            assert_eq!(meta.past_end(), values.past_end);
            assert_eq!(meta.now_before_running(), values.now_before_running);
            assert_eq!(meta.now(), values.now);
            assert_eq!(meta.now_after_running(), values.now_after_running);
            assert_eq!(meta.future_end(), values.future_end);
            assert_eq!(meta.log_exits(), values.log_exits);
            assert_eq!(meta.log_clears(), values.log_clears);
            assert_eq!(direction.discriminant(), values.direction);
        });
    }
}

#[test]
fn traverses_log() {
    let mut meta = RevMeta::new(4);
    meta.update_assert(
        Some(RevQueue::RunNotLog),
        Some(RunValues {
            past_end: 0,
            now_before_running: 0,
            now: 1,
            now_after_running: 1,
            future_end: 1,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunNotLog),
        Some(RunValues {
            past_end: 0,
            now_before_running: 1,
            now: 2,
            now_after_running: 2,
            future_end: 2,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 0,
            now_before_running: 2,
            now: 3,
            now_after_running: 3,
            future_end: 3,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 0,
            now_before_running: 3,
            now: 4,
            now_after_running: 4,
            future_end: 4,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now_before_running: 4,
            now: 5,
            now_after_running: 5,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 1,
            now_before_running: 5,
            now: 5,
            now_after_running: 4,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now_before_running: 4,
            now: 4,
            now_after_running: 3,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 1,
            now_before_running: 3,
            now: 3,
            now_after_running: 2,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now_before_running: 2,
            now: 2,
            now_after_running: 1,
            future_end: 5,
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
            now_before_running: 1,
            now: 2,
            now_after_running: 2,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::ForwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now_before_running: 2,
            now: 3,
            now_after_running: 3,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::ForwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunForwardLog),
        Some(RunValues {
            past_end: 1,
            now_before_running: 3,
            now: 4,
            now_after_running: 4,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::ForwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now_before_running: 4,
            now: 5,
            now_after_running: 5,
            future_end: 5,
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
            now_before_running: 5,
            now: 5,
            now_after_running: 4,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now_before_running: 4,
            now: 4,
            now_after_running: 3,
            future_end: 5,
            log_exits: 0,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(Some(RevQueue::Pause), None);
    meta.update_assert(
        Some(RevQueue::RunNotLog),
        Some(RunValues {
            past_end: 1,
            now_before_running: 3,
            now: 4,
            now_after_running: 4,
            future_end: 4,
            log_exits: 1,
            log_clears: 0,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 1,
            now_before_running: 4,
            now: 4,
            now_after_running: 3,
            future_end: 4,
            log_exits: 1,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 1,
            now_before_running: 3,
            now: 3,
            now_after_running: 2,
            future_end: 4,
            log_exits: 1,
            log_clears: 0,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(Some(RevQueue::ClearThenPause), None);
    meta.update_assert(
        Some(RevQueue::RunNotLog),
        Some(RunValues {
            past_end: 2,
            now_before_running: 2,
            now: 3,
            now_after_running: 3,
            future_end: 3,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 2,
            now_before_running: 3,
            now: 4,
            now_after_running: 4,
            future_end: 4,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 2,
            now_before_running: 4,
            now: 5,
            now_after_running: 5,
            future_end: 5,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
    meta.update_assert(
        Some(RevQueue::RunBackwardLog),
        Some(RunValues {
            past_end: 2,
            now_before_running: 5,
            now: 5,
            now_after_running: 4,
            future_end: 5,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        None,
        Some(RunValues {
            past_end: 2,
            now_before_running: 4,
            now: 4,
            now_after_running: 3,
            future_end: 5,
            log_exits: 0,
            log_clears: 1,
            direction: RevDirection::BackwardLog.discriminant(),
        }),
    );
    meta.update_assert(
        Some(RevQueue::ClearThenRunNotLog),
        Some(RunValues {
            past_end: 3,
            now_before_running: 3,
            now: 4,
            now_after_running: 4,
            future_end: 4,
            log_exits: 0,
            log_clears: 2,
            direction: RevDirection::FWD_DISCRIMINANT,
        }),
    );
}

#[test]
fn contains_returns_expected() {
    let mut meta = RevMeta::new(u64::MAX);
    meta.past_end = 1;
    meta.now = 3;
    meta.future_end = 5;

    assert!(!meta.contains(0), "{meta:#?}");
    assert!(meta.contains(1), "{meta:#?}");
    assert!(meta.contains(2), "{meta:#?}");
    assert!(meta.contains(3), "{meta:#?}");
    assert!(meta.contains(4), "{meta:#?}");
    assert!(meta.contains(5), "{meta:#?}");
    assert!(!meta.contains(6), "{meta:#?}");

    assert!(!meta.past_contains(0), "{meta:#?}");
    assert!(meta.past_contains(1), "{meta:#?}");
    assert!(meta.past_contains(2), "{meta:#?}");
    assert!(!meta.past_contains(3), "{meta:#?}");
    assert!(!meta.past_contains(4), "{meta:#?}");
    assert!(!meta.past_contains(5), "{meta:#?}");
    assert!(!meta.past_contains(6), "{meta:#?}");

    assert!(!meta.future_contains(0), "{meta:#?}");
    assert!(!meta.future_contains(1), "{meta:#?}");
    assert!(!meta.future_contains(2), "{meta:#?}");
    assert!(!meta.future_contains(3), "{meta:#?}");
    assert!(meta.future_contains(4), "{meta:#?}");
    assert!(meta.future_contains(5), "{meta:#?}");
    assert!(!meta.future_contains(6), "{meta:#?}");
}
