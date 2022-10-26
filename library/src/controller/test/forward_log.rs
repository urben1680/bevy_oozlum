use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQueried, ProgressQuery},
};

use super::{tests, Test, CONTROLLER_CONSTS_TIME_STEP_ZERO};

const THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD: [Option<ProgressQuery>; 7] = [
    None,
    None,
    Some(ProgressQuery::BackwardLog),
    None,
    None,
    Some(ProgressQuery::ForwardLog),
    None,
];

const STEP_8_AFTER_FIRST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::ForwardLog {
        after_forward: true,
    },
    time_stamp: Wrapping(2),
    log_len: 3,
    time_step_query: None,
    time_step: 0.0,
    first_ran: false,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(0),
    log_index: 1,
    delayed_commands_len: 0,
    commands_overflows: 0,
};

#[test]
fn processes_none_query() {
    const THREE_FORWARD_THREE_BACKWARD_ZERO_FORWARD: [Option<ProgressQuery>; 6] = [
        None,
        None,
        Some(ProgressQuery::BackwardLog),
        None,
        None,
        Some(ProgressQuery::ForwardLog),
    ];

    const STEP_7_CHECK: DebugLog = DebugLog {
        progress_current: Progress::ForwardLog {
            after_forward: false,
        },
        time_stamp: Wrapping(1),
        log_len: 3,
        time_step_query: None,
        time_step: 0.0,
        first_ran: false,
        progress_query: None,
        forget: Wrapping(0),
        to_time_stamp: Wrapping(0),
        log_index: 2,
        delayed_commands_len: 0,
        commands_overflows: 0,
    };

    const STEP_9_CHECK: DebugLog = DebugLog {
        progress_current: Progress::ForwardLog {
            after_forward: true,
        },
        time_stamp: Wrapping(3),
        log_len: 3,
        time_step_query: None,
        time_step: 0.0,
        first_ran: false,
        progress_query: None,
        forget: Wrapping(0),
        to_time_stamp: Wrapping(0),
        log_index: 0,
        delayed_commands_len: 0,
        commands_overflows: 0,
    };

    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ZERO_FORWARD,
        [
            Test {
                //#1
                after_first_check: DebugLog {
                    progress_query: None,
                    ..STEP_7_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardLog {
                        after_forward: true,
                    },
                    ..STEP_7_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: STEP_8_AFTER_FIRST_CHECK,
                after_last_check: STEP_8_AFTER_FIRST_CHECK,
                ..Default::default()
            },
            Test {
                //#3
                after_first_check: STEP_9_CHECK,
                after_last_check: DebugLog {
                    progress_current: Progress::Pause {
                        after_forward_if_log: Some(true),
                    },
                    ..STEP_9_CHECK
                },
                ..Default::default()
            },
        ],
    )
}
