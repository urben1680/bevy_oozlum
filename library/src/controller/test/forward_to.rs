use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQueried, ProgressQuery},
};

use super::{tests, Test, CONTROLLER_CONSTS_TIME_STEP_ZERO};

const PROGRESS_FORWARD_TO_TO_3: [Option<ProgressQuery>; 1] = [Some(ProgressQuery::ForwardTo(Wrapping(3)))];
const TIME_STAMP_2_AFTER_FIRST_CHECK: DebugLog = DebugLog{
    current: Progress::ForwardTo {
        after_forward_if_init: Some(true),
    },
    time_stamp: Wrapping(2),
    log_len: 2,
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
const TIME_STAMP_2_AFTER_LAST_CHECK: DebugLog = DebugLog{
    current: Progress::ForwardTo {
        after_forward_if_init: None,
    },
    time_stamp: Wrapping(2),
    log_len: 2,
    time_step_query: None,
    time_step: 0.0,
    first_ran: false,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(3),
    log_index: 0,
    delayed_commands_len: 1,
    commands_overflows: 0,
};
const TIME_STAMP_3_AFTER_FIRST_CHECK: DebugLog = DebugLog{
    current: Progress::ForwardTo {
        after_forward_if_init: None,
    },
    time_stamp: Wrapping(3),
    log_len: 3,
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
const TIME_STAMP_3_AFTER_LAST_CHECK: DebugLog = DebugLog{
    current: Progress::Forward {
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
                    ..TIME_STAMP_2_AFTER_FIRST_CHECK
                },
                after_last_check: TIME_STAMP_2_AFTER_LAST_CHECK,
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: TIME_STAMP_3_AFTER_FIRST_CHECK,
                after_last_check: TIME_STAMP_3_AFTER_LAST_CHECK,
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::Forward.into()],
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::Forward),
                    ..TIME_STAMP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Forward),
                    ..TIME_STAMP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Forward),
                    ..TIME_STAMP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    ..TIME_STAMP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_to_not_future() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(3)).into()],
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(3), queried: Wrapping(1) }),
                    ..TIME_STAMP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(3), queried: Wrapping(1) }),
                    ..TIME_STAMP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(3), queried: Wrapping(1) }),
                    ..TIME_STAMP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    ..TIME_STAMP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_to_one_tick() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(4)).into()],
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(4), queried: Wrapping(1) }),
                    ..TIME_STAMP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(4), queried: Wrapping(1) }),
                    ..TIME_STAMP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(4), queried: Wrapping(1) }),
                    ..TIME_STAMP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    ..TIME_STAMP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_to_two_ticks() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(5)).into()],
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(5), queried: Wrapping(1) }),
                    ..TIME_STAMP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(5), queried: Wrapping(1) }),
                    ..TIME_STAMP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo { to_time_stamp: Wrapping(5), queried: Wrapping(1) }),
                    ..TIME_STAMP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: None,
                    to_time_stamp: Wrapping(5),
                    delayed_commands_len: 2,
                    ..TIME_STAMP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardLog.into()],
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardLog),
                    ..TIME_STAMP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardLog),
                    ..TIME_STAMP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardLog),
                    ..TIME_STAMP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    current: Progress::Pause { after_forward_if_log: Some(true) },
                    progress_query: None,
                    ..TIME_STAMP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}


/*
todo: query log_to

#[test]
fn forward_processes_query_backward_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_TO_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::BackwardLog),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLog), //significant check
                    log_len: 2,
                    time_stamp: 1,
                    to_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLog),
                    log_len: 2,
                    time_stamp: 2,
                    to_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLog),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::BackwardLog, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn forward_processes_query_backward_log_end() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_TO_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::BackwardLogEnd),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLogEnd), //significant check
                    log_len: 2,
                    time_stamp: 1,
                    to_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLogEnd),
                    log_len: 2,
                    time_stamp: 2,
                    to_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLogEnd),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::BackwardLogEnd, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    to_init: true, //significant check
                    delayed_commands_len: 1,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn forward_processes_query_pause() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_TO_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::Pause),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause), //significant check
                    log_len: 2,
                    time_stamp: 1,
                    to_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 2,
                    time_stamp: 2,
                    to_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Pause, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn forward_processes_query_pause_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_TO_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::PauseLog),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog), //significant check
                    log_len: 2,
                    time_stamp: 1,
                    to_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog),
                    log_len: 2,
                    time_stamp: 2,
                    to_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::PauseLog, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn forward_processes_query_overwritten() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_TO_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::Pause),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 2,
                    time_stamp: 1,
                    to_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 2,
                    time_stamp: 2,
                    to_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: Some(Progress::PauseLog),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog), //significant check
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::PauseLog,
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                }),
            },
        ],
    );
}
*/
