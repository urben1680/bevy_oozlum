use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQueried, ProgressQuery},
};

use super::{tests, Control, Test, TEST_CONTROLLER_CONSTS, TestAfter};

const PROGRESS_FORWARD_FAST_TO_3: [Control; 1] = [Control {
    progress_query: Some(ProgressQuery::ForwardFast {
        to_time_stamp: Wrapping(3),
    }),
    time_step_query: None,
}];

#[test]
fn processes_none_query() {
    tests(
        TEST_CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        [
            Test {
                //#1
                after_first_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: Some(true),
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: None,
                    time_stamp: Wrapping(2),
                    log_len: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: None,
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: None,
                    time_stamp: Wrapping(2),
                    log_len: 2,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: None,
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: None,
                    time_stamp: Wrapping(3),
                    log_len: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
                after_last_check: DebugLog {
                    current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    time_stamp: Wrapping(3),
                    log_len: 3,
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward() {
    tests(
        TEST_CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        [
            Test {
                //#1
                before_first_control: Control {
                    progress_query: Some(ProgressQuery::Forward),
                    ..Default::default()
                },
                after_first_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: Some(true),
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: Some(ProgressQueried::Forward),
                    time_stamp: Wrapping(2),
                    log_len: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: None,
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: Some(ProgressQueried::Forward),
                    time_stamp: Wrapping(2),
                    log_len: 2,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: None,
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: Some(ProgressQueried::Forward),
                    time_stamp: Wrapping(3),
                    log_len: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
                after_last_check: DebugLog {
                    current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    time_stamp: Wrapping(3),
                    log_len: 3,
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_fast_zero_ticks() {
    tests(
        TEST_CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        [
            Test {
                after_first: TestAfter{
                    control: Some(Control{
                        progress_query: Some(ProgressQuery::ForwardFast {
                            to_time_stamp: Wrapping(2),
                        }),
                        ..Default::default()
                    }),
                    check: Some(DebugLog {
                        current: Progress::ForwardFast {
                            after_forward_if_init: Some(true),
                        },
                        forward_fast_limit: Wrapping(3),
                        progress_query: Some(ProgressQueried::ForwardFast {
                            to_time_stamp: Wrapping(2),
                            queried: Wrapping(1),
                        }),
                        time_stamp: Wrapping(2),
                        log_len: 2,
                        delayed_commands_len: 2,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                after_last: TestAfter{
                    check: Some(DebugLog {
                        current: Progress::ForwardFast {
                            after_forward_if_init: None,
                        },
                        forward_fast_limit: Wrapping(3),
                        progress_query: Some(ProgressQueried::ForwardFast {
                            to_time_stamp: Wrapping(2),
                            queried: Wrapping(1),
                        }),
                        time_stamp: Wrapping(2),
                        log_len: 2,
                        delayed_commands_len: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            }
        ],
    )
}

#[test]
fn processes_query_forward_fast_zero_ticks() {
    tests(
        TEST_CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        [
            Test {
                //#1
                before_first_control: Control {
                    progress_query: Some(ProgressQuery::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    ..Default::default()
                },
                after_first_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: Some(true),
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: Some(ProgressQueried::ForwardFast {
                        to_time_stamp: Wrapping(2),
                        queried: Wrapping(1),
                    }),
                    time_stamp: Wrapping(2),
                    log_len: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                after_last_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: None,
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: Some(ProgressQueried::ForwardFast {
                        to_time_stamp: Wrapping(2),
                        queried: Wrapping(1),
                    }),
                    time_stamp: Wrapping(2),
                    log_len: 2,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    current: Progress::ForwardFast {
                        after_forward_if_init: None,
                    },
                    forward_fast_limit: Wrapping(3),
                    progress_query: Some(ProgressQueried::ForwardFast {
                        to_time_stamp: Wrapping(2),
                        queried: Wrapping(1),
                    }),
                    time_stamp: Wrapping(3),
                    log_len: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
                after_last_check: DebugLog {
                    current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    time_stamp: Wrapping(3),
                    log_len: 3,
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
    )
}

/*

#[test]
fn forward_processes_query_forward_fast_closer() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    log_len: 2,
                    time_stamp: 1,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
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
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Forward, //significant check
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
fn forward_processes_query_forward_fast_equal() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    log_len: 2,
                    time_stamp: 1,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
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
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Forward, //significant check
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
fn forward_processes_query_forward_fast_one_tick_further() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(4),
                    }),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(4),
                    }),
                    log_len: 2,
                    time_stamp: 1,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(4),
                    }),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
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
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(4),
                    }),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Forward, //significant check
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
fn forward_processes_query_forward_fast_two_ticks_further() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }), //significant check
                    log_len: 2,
                    time_stamp: 1,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
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
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }),
                    log_len: 3,
                    time_stamp: 2,
                    delayed_commands_len: 2,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    fast_init: true, //significant check
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn forward_processes_query_forward_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::ForwardLog),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLog), //significant check
                    log_len: 2,
                    time_stamp: 1,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLog),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
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
                    progress_query: Some(Progress::ForwardLog),
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
fn forward_processes_query_forward_log_end() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::ForwardLogEnd),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLogEnd), //significant check
                    log_len: 2,
                    time_stamp: 1,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLogEnd),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
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
                    progress_query: Some(Progress::ForwardLogEnd),
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
fn forward_processes_query_backward_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
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
                    fast_init: true,
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
                    fast_init: false,
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
        PROGRESS_FORWARD_FAST_TO_3,
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
                    fast_init: true,
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
                    fast_init: false,
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
                    fast_init: true, //significant check
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
        PROGRESS_FORWARD_FAST_TO_3,
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
                    fast_init: true,
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
                    fast_init: false,
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
        PROGRESS_FORWARD_FAST_TO_3,
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
                    fast_init: true,
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
                    fast_init: false,
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
        PROGRESS_FORWARD_FAST_TO_3,
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
                    fast_init: true,
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
                    fast_init: false,
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
