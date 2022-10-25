use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQueried, ProgressQuery},
};

use super::{tests, Test, CONTROLLER_CONSTS_TIME_STEP_ZERO};

const THREE_FORWARD: [Option<ProgressQuery>; 3] = [None; 3];
const TIME_STAMP_4_AFTER_FIRST_CHECK: DebugLog = DebugLog{
    current: Progress::Forward {
        after_forward: true,
    },
    time_stamp: Wrapping(4),
    log_len: 4,
    time_step_query: None,
    time_step: 0.0,
    first_ran: true,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(0),
    log_index: 0,
    delayed_commands_len: 0,
    commands_overflows: 0,
};
const TIME_STAMP_4_AFTER_LAST_CHECK: DebugLog = DebugLog{
    current: Progress::Forward {
        after_forward: true,
    },
    progress_query: None,
    time_stamp: Wrapping(4),
    log_len: 4,
    time_step_query: None,
    time_step: 0.0,
    first_ran: false,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(0),
    log_index: 0,
    delayed_commands_len: 0,
    commands_overflows: 0,
};

#[test]
fn processes_none_query() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            after_first_check: DebugLog {
                progress_query: None,
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: TIME_STAMP_4_AFTER_LAST_CHECK,
            ..Default::default()
        }],
    )
}

#[test]
fn processes_query_forward() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::Forward.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::Forward),
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                ..TIME_STAMP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    )
}

#[test]
fn processes_query_forward_to_not_future() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(4)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(4), queried: Wrapping(3) }),
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                ..TIME_STAMP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    )
}

#[test]
fn processes_query_forward_to_one_tick() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(5)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(5), queried: Wrapping(3) }),
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                ..TIME_STAMP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    )
}

#[test]
fn processes_query_forward_to_two_ticks() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(6)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(6), queried: Wrapping(3) }),
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                current: Progress::ForwardTo { after_forward_if_init: Some(true) },
                progress_query: None,
                delayed_commands_len: 2,
                to_time_stamp: Wrapping(6),
                ..TIME_STAMP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    )
}

#[test]
fn processes_query_forward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::ForwardLog.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardLog),
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                current: Progress::Pause { after_forward_if_log: Some(true) },
                progress_query: None,
                ..TIME_STAMP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    )
}

#[test]
fn processes_query_backward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::BackwardLog.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::BackwardLog),
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                current: Progress::BackwardLog { after_backward: false },
                progress_query: None,
                ..TIME_STAMP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    )
}

//todo query log_to

#[test]
fn processes_query_pause() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD,
        [Test {
            before_first_commands: vec![ProgressQuery::Pause.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::Pause),
                ..TIME_STAMP_4_AFTER_FIRST_CHECK
            },
            after_last_check: DebugLog {
                current: Progress::Pause { after_forward_if_log: None },
                progress_query: None,
                ..TIME_STAMP_4_AFTER_LAST_CHECK
            },
            ..Default::default()
        }],
    )
}
