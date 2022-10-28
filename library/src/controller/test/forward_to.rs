use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQuery},
};

use super::{tests, Test, CONTROLLER_CONSTS_TIME_STEP_ZERO};

const PROGRESS_FORWARD_TO_TO_3: [Option<ProgressQuery>; 1] =
    [Some(ProgressQuery::ForwardTo(Wrapping(3)))];

const STEP_2_AFTER_FIRST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::ForwardTo {
        init: true,
    },
    time_stamp: Wrapping(2),
    log_len: 2,
    log_at_index_len: 0,
    time_step_query: None,
    time_step: 0.0,
    first_ran: true,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(3),
    log_index: 0,
    delayed_commands_len: 2,
    commands_overflows: 0,
};
const STEP_2_AFTER_LAST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::ForwardTo{
        init: false
    },
    delayed_commands_len: 1,
    log_at_index_len: 2,
    ..STEP_2_AFTER_FIRST_CHECK
};
const STEP_3_AFTER_FIRST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::ForwardTo {
        init: false,
    },
    time_stamp: Wrapping(3),
    log_len: 3,
    log_at_index_len: 0,
    time_step_query: None,
    time_step: 0.0,
    first_ran: true,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(3),
    log_index: 0,
    delayed_commands_len: 1,
    commands_overflows: 0,
};
const STEP_3_AFTER_LAST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::Forward,
    to_time_stamp: Wrapping(0),
    delayed_commands_len: 0,
    log_at_index_len: 3,
    ..STEP_3_AFTER_FIRST_CHECK
};

#[test]
fn processes_none_query() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                after_first_check: DebugLog {
                    progress_query: None,
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: STEP_2_AFTER_LAST_CHECK,
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: STEP_3_AFTER_FIRST_CHECK,
                after_last_check: STEP_3_AFTER_LAST_CHECK,
                ..Default::default()
            },
        ],
    );
}

#[test]
#[should_panic = "Invalid progress query: Forward, error: ForwardToOrLogTo"]
fn processes_query_forward() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::Forward.into()],
                ..Default::default()
            },
        ],
    );
}

#[test]
#[should_panic = "Invalid progress query: ForwardTo(3), error: ForwardToOrLogTo"]
fn processes_query_forward_to() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(3)).into()],
                ..Default::default()
            }
        ],
    );
}

#[test]
#[should_panic = "Invalid progress query: ForwardLog, error: ForwardToOrLogTo"]
fn processes_query_forward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardLog.into()],
                ..Default::default()
            },
        ],
    );
}

#[test]
#[should_panic = "Invalid progress query: BackwardLog, error: ForwardToOrLogTo"]
fn processes_query_backward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::BackwardLog.into()],
                ..Default::default()
            },
        ],
    );
}

#[test]
#[should_panic = "Invalid progress query: LogTo(3), error: ForwardToOrLogTo"]
fn processes_query_log_to() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::LogTo(Wrapping(3)).into()],
                ..Default::default()
            },
        ],
    );
}

#[test]
#[should_panic = "Invalid progress query: Pause, error: ForwardToOrLogTo"]
fn processes_query_pause() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::Pause.into()],
                ..Default::default()
            },
        ],
    );
}