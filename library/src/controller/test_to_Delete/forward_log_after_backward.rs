use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

#[test]
fn forward_log_after_backward_query_none_to_end() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: None,
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause {
                after_forward_if_log: Some(true),
            },
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_forward() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::Forward),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::LogClose {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_forward_to_present() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(1))),
            commands: vec![Command::Redo],
        },
    ]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(2)..=Wrapping(3),
    )));
}

#[test]
fn forward_log_after_backward_query_forward_one_tick() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(2))),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::LogClose {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_forward_two_ticks() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(3))),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::LogClose {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::ForwardTo { init: true },
            progress_query: None,
            commands: vec![],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_forward_to_too_many_ticks() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(4))),
            commands: vec![Command::Redo],
        },
    ]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(2)..=Wrapping(3),
    )));
}

#[test]
fn forward_log_after_backward_query_forward_log_at_end(){
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause { after_forward_if_log: Some(true) },
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_forward_log_not_at_end(){
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: true,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::ForwardLog {
                after_forward: true,
            },
            progress_query: None,
            commands: vec![Command::Redo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_backward_log(){
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: true,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog { after_backward: false },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_log_to_past() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::LogFastTo(Wrapping(0))),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLogFast {
                after_backward_if_init: Some(false),
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_log_to_present() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::LogFastTo(Wrapping(1))),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause {
                after_forward_if_log: Some(true),
            },
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_log_to_future() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: true,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::LogFastTo(Wrapping(2))),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::ForwardLogFast {
                after_forward_if_init: Some(true),
            },
            progress_query: None,
            commands: vec![Command::Redo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_log_after_backward_query_log_to_too_many_ticks() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::LogFastTo(Wrapping(2))),
            commands: vec![Command::Redo],
        },
    ]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(0)..=Wrapping(1),
    )));
}

#[test]
fn forward_log_after_backward_query_pause() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::Pause),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause {
                after_forward_if_log: Some(true),
            },
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}
