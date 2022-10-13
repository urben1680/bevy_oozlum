use std::num::Wrapping;

use crate::controller::{consts::CONTROLLER_CONSTS, progress::Progress};

use super::{Test, TestAssert, TestControl};

const PROGRESS_FORWARD_FAST_TO_3: [TestControl; 1] = [TestControl {
    progress_query: Some(Progress::ForwardFast {
        to_time_stamp: Wrapping(3),
    }),
    time_step_query: None,
}];

#[test]
fn forward_processes_none_query() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: None,
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: None,
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::Forward,
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
            },
        ],
    );
}

#[test]
fn forward_processes_query_forward() {
    Test::tests(
        CONTROLLER_CONSTS,
        PROGRESS_FORWARD_FAST_TO_3,
        vec![
            Test {
                //test_index: 0
                control: TestControl {
                    progress_query: Some(Progress::Forward),
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Forward), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Forward),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Forward),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::Forward, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
            },
        ],
    );
}

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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
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
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(2),
                    }),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::Forward, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
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
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    }),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::Forward, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(4),
                    }),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
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
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(4),
                    }),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::Forward, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
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
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(5),
                    }, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    fast_init: true, //significant check
                    delayed_commands_len: 3,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLog), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLog),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLog),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::PauseLog, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLogEnd), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLogEnd),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::ForwardLogEnd),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::PauseLog, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLog), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLog),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLog),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::BackwardLog, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLogEnd), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLogEnd),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::BackwardLogEnd),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::BackwardLogEnd, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    fast_init: true, //significant check
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::Pause, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog), //significant check
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: None,
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog),
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::PauseLog, //significant check
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
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
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: true,
                    delayed_commands_len: 3,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::Pause),
                    log_len: 2,
                    time_stamp: 2,
                    fast_init: false,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
            },
            Test {
                //test_index: 1
                control: TestControl {
                    progress_query: Some(Progress::PauseLog),
                    time_step_query: None,
                },
                assert_at_update: TestAssert {
                    progress: Progress::ForwardFast {
                        to_time_stamp: Wrapping(3),
                    },
                    progress_query: Some(Progress::PauseLog), //significant check
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 2,
                    ..Default::default()
                },
                assert_at_end: TestAssert {
                    progress: Progress::PauseLog,
                    progress_query: None,
                    log_len: 3,
                    time_stamp: 3,
                    delayed_commands_len: 1,
                    ..Default::default()
                },
            },
        ],
    );
}
