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

const STEP_8_CHECK: DebugLog = DebugLog {
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
                after_first_check: STEP_8_CHECK,
                after_last_check: STEP_8_CHECK,
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
    );
}

#[test]
fn processes_query_forward() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::Forward.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::Forward),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::Forward),
                ..STEP_8_CHECK
            },
            ..Default::default()
        },
        Test {
            //#2
            after_first_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::Forward),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Forward { after_forward: true },
                progress_query: None,
                log_index: 0,
                log_len: 2,
                time_stamp: Wrapping(2),
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_to_not_future(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(2)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(2), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(2), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            ..Default::default()
        },
        Test {
            //#2
            after_first_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(2), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Forward { after_forward: true },
                progress_query: None,
                log_index: 0,
                log_len: 2,
                time_stamp: Wrapping(2),
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_to_one_tick(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(3)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(3), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(3), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            ..Default::default()
        },
        Test {
            //#2
            after_first_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(3), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Forward { after_forward: true },
                progress_query: None,
                log_index: 0,
                log_len: 2,
                time_stamp: Wrapping(2),
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_to_two_ticks(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(4)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(4), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(4), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            ..Default::default()
        },
        Test {
            //#2
            after_first_check: DebugLog {
                progress_current: Progress::LogClose { after_forward: true },
                progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(4), queried: Wrapping(1) }),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::ForwardTo { after_forward_if_init: Some(true) },
                progress_query: None,
                log_index: 0,
                log_len: 2,
                time_stamp: Wrapping(2),
                to_time_stamp: Wrapping(4),
                delayed_commands_len: 2,
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_log(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::ForwardLog.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::ForwardLog),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::ForwardLog { after_forward: true },
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_backward_log(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::BackwardLog.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::BackwardLog),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::BackwardLog { after_backward: false },
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_log_to_now(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::LogTo(Wrapping(2)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::LogTo(Wrapping(2))),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Pause { after_forward_if_log: Some(true) },
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_log_to_past(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::LogTo(Wrapping(1)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::LogTo(Wrapping(1))),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::BackwardLogTo { after_backward_if_init: Some(false) },
                to_time_stamp: Wrapping(1),
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_log_to_future(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::LogTo(Wrapping(3)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::LogTo(Wrapping(3))),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::ForwardLogTo { after_forward_if_init: Some(true) },
                to_time_stamp: Wrapping(3),
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}

#[test]
#[should_panic(
    expected = "`ProgressQueried::LogTo(Wrapping(4))` out of range of `Wrapping(0)..=Wrapping(3)`."
)]
fn processes_query_log_to_invalid(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::LogTo(Wrapping(4)).into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::LogTo(Wrapping(4))),
                ..STEP_8_CHECK
            },
            after_last_check: STEP_8_CHECK,
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_pause(){
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        THREE_FORWARD_THREE_BACKWARD_ONE_FORWARD,
        [Test {
            //#1
            before_first_commands: vec![ProgressQuery::Pause.into()],
            after_first_check: DebugLog {
                progress_query: Some(ProgressQueried::Pause),
                ..STEP_8_CHECK
            },
            after_last_check: DebugLog {
                progress_current: Progress::Pause { after_forward_if_log: Some(true) },
                ..STEP_8_CHECK
            },
            ..Default::default()
        }],
    );
}