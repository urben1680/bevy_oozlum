use std::num::Wrapping;

use crate::controller::{consts::CONTROLLER_CONSTS, progress::Progress};

use super::{Test, TestAssert, TestControl};

const THREE_FORWARD: [TestControl; 3] = [TestControl {
    progress_query: None,
    time_step_query: None,
}; 3];

#[test]
fn processes_none_query() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: None,
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: None,
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_forward() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::Forward),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::Forward),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_forward_fast_one_tick() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::ForwardFast {
                    to_time_stamp: Wrapping(5),
                }),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::ForwardFast {
                    to_time_stamp: Wrapping(5),
                }),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_forward_fast_two_ticks() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::ForwardFast {
                    to_time_stamp: Wrapping(6),
                }),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::ForwardFast {
                    to_time_stamp: Wrapping(6),
                }),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::ForwardFast {
                    to_time_stamp: Wrapping(6),
                },
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                fast_init: true,
                delayed_commands_len: 3,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_forward_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::ForwardLog),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::ForwardLog),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::PauseLog,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_forward_log_end() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::ForwardLogEnd),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::ForwardLogEnd),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::PauseLog,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_backward_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::BackwardLog),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::BackwardLog),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::BackwardLog,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_backward_log_end() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::BackwardLogEnd),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::BackwardLogEnd),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::BackwardLogEnd,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                fast_init: true,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_pause() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::Pause),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::Pause),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::Pause,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}

#[test]
fn processes_query_pause_log() {
    Test::tests(
        CONTROLLER_CONSTS,
        THREE_FORWARD,
        vec![Test {
            //test_index: 0
            control: TestControl {
                progress_query: Some(Progress::PauseLog),
                time_step_query: None,
            },
            assert_at_update: Box::new(TestAssert {
                progress: Progress::Forward,
                progress_query: Some(Progress::PauseLog),
                log_len: 4,
                time_stamp: 3,
                ..Default::default()
            }),
            assert_at_end: Box::new(TestAssert {
                progress: Progress::PauseLog,
                progress_query: None,
                log_len: 4,
                time_stamp: 4,
                ..Default::default()
            }),
        }],
    );
}
