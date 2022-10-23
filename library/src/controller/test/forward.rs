use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQueried, ProgressQuery},
};

use super::{tests, Control, Test, TEST_CONTROLLER_CONSTS};

const THREE_FORWARD: [Control; 3] = [Control {
    progress_query: None,
    time_step_query: None,
}; 3];

#[test]
fn processes_none_query() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                time_stamp: Wrapping(4),
                log_len: 4,
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                time_stamp: Wrapping(4),
                log_len: 4,
                ..Default::default()
            },
            ..Default::default()
        }],
    )
}

#[test]
fn processes_query_forward() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::Forward),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::Forward),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_fast_zero_ticks() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::ForwardFast {
                    to_time_stamp: Wrapping(4),
                }),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::ForwardFast {
                    to_time_stamp: Wrapping(4),
                    queried: Wrapping(3)
                }),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_fast_one_tick() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::ForwardFast {
                    to_time_stamp: Wrapping(5),
                }),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::ForwardFast {
                    to_time_stamp: Wrapping(5),
                    queried: Wrapping(3)
                }),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_fast_two_ticks() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::ForwardFast {
                    to_time_stamp: Wrapping(6),
                }),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::ForwardFast {
                    to_time_stamp: Wrapping(6),
                    queried: Wrapping(3)
                }),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::ForwardFast { 
                    after_forward_if_init: Some(true) 
                },
                forward_fast_limit: Wrapping(6),
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                delayed_commands_len: 2,
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_log() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::ForwardLog),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::ForwardLog),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::Pause { after_forward_if_log: Some(true) },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_forward_log_end() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::ForwardLogEnd),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::ForwardLogEnd),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::Pause { after_forward_if_log: Some(true) },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_backward_log() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::BackwardLog),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::BackwardLog),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::BackwardLog { after_backward: false },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_backward_log_end() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::BackwardLogEnd),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::BackwardLogEnd),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::BackwardLogEnd { after_backward_if_init: Some(false) },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}

#[test]
fn processes_query_pause() {
    tests(
        TEST_CONTROLLER_CONSTS,
        THREE_FORWARD,
        [Test {
            //#1
            before_first: Control {
                progress_query: Some(ProgressQuery::Pause),
                ..Default::default()
            },
            after_first: DebugLog {
                current: Progress::Forward {
                    after_forward: true,
                },
                progress_query: Some(ProgressQueried::Pause),
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            after_last: DebugLog {
                current: Progress::Pause { after_forward_if_log: None },
                progress_query: None,
                log_len: 4,
                time_stamp: Wrapping(4),
                ..Default::default()
            },
            ..Default::default()
        }],
    );
}