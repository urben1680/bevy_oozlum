/*
- forward log / backward log
-- keeps running at this progress
-- calls commands as expected
-- stops at the end end and changes to log pause
-- triggers log end when reacting on non-log progress query (test all)
-- reacts other log progresses immediately without triggering log end (test all)
*/

use std::num::Wrapping;

use crate::controller::{consts::CONTROLLER_CONSTS, progress::Progress};

use super::{Test, TestAssert, TestControl};

const TWO_FORWARD_THEN_BACK_QUERY: [TestControl; 2] = [
    TestControl {
        progress_query: None,
        time_step_query: None,
    },
    TestControl {
        progress_query: Some(Progress::BackwardLog),
        time_step_query: None,
    },
];

const ONE_FORWARD_THEN_BACK_QUERY: [TestControl; 1] = [TestControl {
    progress_query: Some(Progress::BackwardLog),
    time_step_query: None,
}];

const THREE_FORWARD_THEN_BACK_QUERY: [TestControl; 3] = [
    TestControl {
        progress_query: None,
        time_step_query: None,
    },
    TestControl {
        progress_query: None,
        time_step_query: None,
    },
    TestControl {
        progress_query: Some(Progress::BackwardLog),
        time_step_query: None,
    },
];

#[test]
fn processes_none_query_to_end() {
    Test::tests(
        CONTROLLER_CONSTS,
        TWO_FORWARD_THEN_BACK_QUERY,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::BackwardLog,
                    progress_query: None,
                    log_len: 2,
                    log_index: 0,
                    time_stamp: 1,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::BackwardLog,
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    backward: true,
                    time_stamp: 1,
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
                    progress: Progress::BackwardLog,
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    backward: true,
                    time_stamp: 0,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::PauseLog,
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    backward: true,
                    time_stamp: 0,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn processes_query_forward() {
    Test::tests(
        CONTROLLER_CONSTS,
        TWO_FORWARD_THEN_BACK_QUERY,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::Forward),
                    time_step_query: None,
                },
                assert_at_update: Box::new(TestAssert {
                    progress: Progress::BackwardLog,
                    progress_query: Some(Progress::Forward),
                    log_len: 2,
                    log_index: 0,
                    time_stamp: 1,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Forward,
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    time_stamp: 1,
                    log_end: true,
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
                    progress: Progress::Forward,
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    time_stamp: 1,
                    log_end: true,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Forward,
                    progress_query: None,
                    log_len: 1,
                    log_index: 0,
                    time_stamp: 1,
                    log_end: false,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn processes_query_forward_fast_zero_ticks() {
    Test::tests(
        CONTROLLER_CONSTS,
        TWO_FORWARD_THEN_BACK_QUERY,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::ForwardFast {
                    to_time_stamp: Wrapping(1),
                }),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::BackwardLog,
                progress_query: Some(Progress::ForwardFast {
                    to_time_stamp: Wrapping(1),
                }),
                log_len: 2,
                log_index: 0,
                time_stamp: 1,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::BackwardLog,
                progress_query: None,
                log_len: 2,
                log_index: 1,
                time_stamp: 1,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_forward_fast_one_tick() {
    Test::tests(
        CONTROLLER_CONSTS,
        TWO_FORWARD_THEN_BACK_QUERY,
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
                    progress: Progress::BackwardLog,
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    log_len: 2,
                    log_index: 0,
                    time_stamp: 1,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Forward,
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    time_stamp: 1,
                    log_end: true,
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
                    progress: Progress::Forward,
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    time_stamp: 1,
                    log_end: true,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::Forward,
                    progress_query: None,
                    log_len: 1,
                    log_index: 0,
                    time_stamp: 1,
                    log_end: false,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn processes_query_forward_fast_two_ticks() {
    Test::tests(
        CONTROLLER_CONSTS,
        TWO_FORWARD_THEN_BACK_QUERY,
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
                    progress: Progress::BackwardLog,
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    log_len: 2,
                    log_index: 0,
                    time_stamp: 1,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    time_stamp: 1,
                    log_end: true,
                    fast_init: true,
                    delayed_commands_len: 3,
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
                    progress_query: None,
                    log_len: 2,
                    log_index: 1,
                    time_stamp: 1,
                    log_end: true,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
                assert_at_end: Box::new(TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: None,
                    log_len: 1,
                    log_index: 0,
                    time_stamp: 1,
                    log_end: false,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                }),
            },
        ],
    );
}

#[test]
fn processes_query_backward_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        TWO_FORWARD_THEN_BACK_QUERY,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::BackwardLog),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::BackwardLog,
                progress_query: Some(Progress::BackwardLog),
                log_len: 2,
                log_index: 0,
                time_stamp: 1,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::BackwardLog,
                progress_query: None,
                log_len: 2,
                log_index: 1,
                time_stamp: 1,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_backward_log_end_zero_ticks() {
    /*
    Problem:
    Bei log_index = 0 und log.len() = 1 kann man nicht erkennen ob man vor oder zurück im log gehen kann

    back: log_index zählt hoch, sonst runter

    backward_log && log_index == log.len() - 1  STOP
                                           - 2  LAST GO
    forward_log && log_index == 0               STOP
                                1               LAST GO


    index   3   2   1   0
    
    */
    Test::tests(
        CONTROLLER_CONSTS,
        ONE_FORWARD_THEN_BACK_QUERY,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::BackwardLogEnd),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::BackwardLog,
                progress_query: Some(Progress::BackwardLogEnd),
                log_len: 1,
                log_index: 0,
                time_stamp: 0,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::PauseLog,
                progress_query: None,
                log_len: 1,
                log_index: 0,
                time_stamp: 0,
                ..Default::default()
            }),
        }],
    );
}
