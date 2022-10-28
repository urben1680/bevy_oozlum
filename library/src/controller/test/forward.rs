use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQuery},
};

use super::{tests, Test, CONTROLLER_CONSTS_TIME_STEP_ZERO};

const THREE_FORWARD: [Option<ProgressQuery>; 3] = [None; 3];

const STEP_4_AFTER_FIRST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::Forward,
    time_stamp: Wrapping(4),
    log_len: 4,
    log_at_index_len: 0,
    time_step_query: None,
    time_step: 0.0,
    first_ran: true,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(0),
    log_index: 0,
    delayed_commands_len: 1,
    commands_overflows: 0,
};
const STEP_4_AFTER_LAST_CHECK: DebugLog = DebugLog {
    delayed_commands_len: 0,
    log_at_index_len: 4,
    ..STEP_4_AFTER_FIRST_CHECK
};

#[test]
fn processes_none_query() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            after_first_check: DebugLog {
                progress_query: None,
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: STEP_4_AFTER_LAST_CHECK,
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::Forward.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::Forward),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Forward,
                progress_query: None,
                delayed_commands_len: 0,
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
#[should_panic = "Invalid progress query: ForwardTo(4), error: QueryFortwardToPresent"]
fn processes_query_forward_to_present() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(4)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::ForwardTo(Wrapping(4))),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_to_one_tick() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(5)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::ForwardTo(Wrapping(5))),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Forward,
                progress_query: None,
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_to_two_ticks() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(6)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::ForwardTo(Wrapping(6))),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::ForwardTo {
                    init: true,
                },
                progress_query: None,
                delayed_commands_len: 2,
                to_time_stamp: Wrapping(6),
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardLog.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::ForwardLog),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Pause {
                    after_forward_if_log: Some(true),
                },
                progress_query: None,
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_backward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::BackwardLog.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::BackwardLog),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::BackwardLog {
                    after_backward: false,
                },
                progress_query: None,
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_log_to_now() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::LogTo(Wrapping(4)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::LogTo(Wrapping(4))),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Pause {
                    after_forward_if_log: Some(true),
                },
                progress_query: None,
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_log_to_past() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::LogTo(Wrapping(3)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::LogTo(Wrapping(3))),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::BackwardLogTo {
                    after_backward_if_init: Some(false),
                },
                progress_query: None,
                to_time_stamp: Wrapping(3),
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
#[should_panic = "Invalid progress query: LogTo(5), error: QueryOutOfRange(0..=4)"]
fn processes_query_log_to_invalid() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::LogTo(Wrapping(5)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::LogTo(Wrapping(5))),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: STEP_4_AFTER_LAST_CHECK,
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_pause() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::Pause.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQuery::Pause),
                ..STEP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Pause {
                    after_forward_if_log: None,
                },
                progress_query: None,
                ..STEP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    );
}
