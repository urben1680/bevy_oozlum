use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

fn forward_to_init_after_backward_query(query: ProgressQuery) {
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
            progress_query: Some(ProgressQuery::LogFastTo(Wrapping(2))),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::ForwardLogFast {
                after_forward_if_init: Some(false),
            },
            progress_query: Some(query),
            commands: vec![Command::Redo],
        },
    ]
    .run(Err(ProgressQueryError::IncompatibleWithCurrent))
}

#[test]
fn forward_to_init_after_backward_query_log_forward() {
    forward_to_init_after_backward_query(ProgressQuery::Forward);
}

#[test]
fn forward_to_init_after_backward_query_forward_to_present() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardFastTo(Wrapping(2)));
}

#[test]
fn forward_to_init_after_backward_query_forward_to_one_tick() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardFastTo(Wrapping(3)));
}

#[test]
fn forward_to_init_after_backward_query_forward_to_two_ticks() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardFastTo(Wrapping(4)));
}

#[test]
fn forward_to_init_after_backward_query_forward_to_too_many_ticks() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardFastTo(Wrapping(5)));
}

#[test]
fn forward_to_init_after_backward_query_forward_log() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardLog);
}

#[test]
fn forward_to_init_after_backward_query_backward_log() {
    forward_to_init_after_backward_query(ProgressQuery::BackwardLog);
}

#[test]
fn forward_to_init_after_backward_query_log_to_past() {
    forward_to_init_after_backward_query(ProgressQuery::LogFastTo(Wrapping(1)));
}

#[test]
fn forward_to_init_after_backward_query_log_to_present() {
    forward_to_init_after_backward_query(ProgressQuery::LogFastTo(Wrapping(2)));
}

#[test]
fn forward_to_init_after_backward_query_log_to_future() {
    forward_to_init_after_backward_query(ProgressQuery::LogFastTo(Wrapping(3)));
}

#[test]
fn forward_to_init_after_backward_query_log_pause() {
    forward_to_init_after_backward_query(ProgressQuery::Pause);
}
