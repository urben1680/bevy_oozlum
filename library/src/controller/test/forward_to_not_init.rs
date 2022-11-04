use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

fn forward_to_not_init_query(query: ProgressQuery) {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(3))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            progress_current: Progress::ForwardTo { init: true },
            progress_query: None,
            commands: vec![],
        },
        Test {
            time_stamp: Some(3),
            progress_current: Progress::ForwardTo { init: false },
            progress_query: Some(query),
            commands: vec![Command::Init, Command::Init],
        },
    ]
    .run(Err(ProgressQueryError::IncompatibleWithCurrent));
}

#[test]
fn forward_to_not_init_query_forward() {
    forward_to_not_init_query(ProgressQuery::Forward);
}

#[test]
fn forward_to_not_init_query_forward_to_present() {
    forward_to_not_init_query(ProgressQuery::ForwardFastTo(Wrapping(3)));
}

#[test]
fn forward_to_not_init_query_forward_to_one_tick() {
    forward_to_not_init_query(ProgressQuery::ForwardFastTo(Wrapping(4)));
}

#[test]
fn forward_to_not_init_query_forward_to_two_ticks() {
    forward_to_not_init_query(ProgressQuery::ForwardFastTo(Wrapping(5)));
}

#[test]
fn forward_to_not_init_query_forward_to_too_many_ticks() {
    forward_to_not_init_query(ProgressQuery::ForwardFastTo(Wrapping(6)));
}

#[test]
fn forward_to_not_init_query_forward_log() {
    forward_to_not_init_query(ProgressQuery::ForwardLog);
}

#[test]
fn forward_to_not_init_query_backward_log() {
    forward_to_not_init_query(ProgressQuery::BackwardLog);
}

#[test]
fn forward_to_not_init_query_log_to_past() {
    forward_to_not_init_query(ProgressQuery::LogFastTo(Wrapping(2)));
}

#[test]
fn forward_to_not_init_query_log_to_present() {
    forward_to_not_init_query(ProgressQuery::LogFastTo(Wrapping(3)));
}

#[test]
fn forward_to_not_init_query_log_to_future() {
    forward_to_not_init_query(ProgressQuery::LogFastTo(Wrapping(4)));
}

#[test]
fn forward_to_not_init_query_pause() {
    forward_to_not_init_query(ProgressQuery::Pause);
}
