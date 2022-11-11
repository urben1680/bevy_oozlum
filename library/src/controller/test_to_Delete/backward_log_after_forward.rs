use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

#[test]
fn backward_log_after_forward_query_none_to_end() {
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
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause {
                after_forward_if_log: Some(false),
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: None,
            commands: vec![Command::Redo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn backward_log_after_forward_query_forward() {
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
            progress_query: Some(ProgressQuery::Forward),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::LogClose {
                after_backward: true,
            },
            progress_query: None,
            commands: vec![Command::UndoFinalize],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn backward_log_after_forward_query_forward_to_present() {
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
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(0))),
            commands: vec![Command::Undo],
        },
    ]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(1)..=Wrapping(2),
    )));
}

#[test]
fn backward_log_after_forward_query_forward_to_one_tick() {
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
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(1))),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::LogClose {
                after_backward: true,
            },
            progress_query: None,
            commands: vec![Command::UndoFinalize],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn backward_log_after_forward_query_forward_to_two_ticks() {
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
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(2))),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::LogClose {
                after_backward: true,
            },
            progress_query: None,
            commands: vec![Command::UndoFinalize],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardTo { init: true },
            progress_query: None,
            commands: vec![],
        },
    ]
    .run(Ok(()));
}

#[test]
fn backward_log_after_forward_query_forward_to_too_many_ticks() {
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
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(3))),
            commands: vec![Command::Undo],
        },
    ]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(1)..=Wrapping(2),
    )));
}

#[test]
fn backward_log_after_forward_query_forward_log() {
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
            progress_current: Progress::ForwardLog { after_forward: false },
            progress_query: None,
            commands: vec![Command::Redo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn backward_log_after_forward_query_backward_log_at_end() {
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
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause { after_forward_if_log: Some(false) },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLog { after_forward: false },
            progress_query: None,
            commands: vec![Command::Redo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn backward_log_after_forward_query_backward_log_not_at_end() {
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
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog { after_backward: true },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}