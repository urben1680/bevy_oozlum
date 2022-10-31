use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

fn forward_to_init_query(query: ProgressQuery) {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardTo(Wrapping(3))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::ForwardTo { init: true },
            progress_query: Some(query),
            commands: vec![],
        },
    ]
    .run(Err(ProgressQueryError::ToProgressActive));
}

fn forward_to_not_init_query(query: ProgressQuery) {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardTo(Wrapping(3))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::ForwardTo { init: true },
            progress_query: None,
            commands: vec![],
        },
        Test {
            time_stamp: 3,
            progress_current: Progress::ForwardTo { init: false },
            progress_query: Some(query),
            commands: vec![Command::Init, Command::Init],
        },
    ]
    .run(Err(ProgressQueryError::ToProgressActive));
}

#[test]
fn forward_to_query_none() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardTo(Wrapping(3))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::ForwardTo { init: true },
            progress_query: None,
            commands: vec![],
        },
        Test {
            time_stamp: 3,
            progress_current: Progress::ForwardTo { init: false },
            progress_query: None,
            commands: vec![Command::Init, Command::Init],
        },
        Test {
            time_stamp: 4,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::RedoFinalize, Command::Init],
        },
        Test {
            time_stamp: 5,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_to_init_query_forward() {
    forward_to_init_query(ProgressQuery::Forward);
}

#[test]
fn forward_to_init_query_forward_to_present() {
    forward_to_init_query(ProgressQuery::ForwardTo(Wrapping(2)));
}

#[test]
fn forward_to_init_query_forward_to_1_tick() {
    forward_to_init_query(ProgressQuery::ForwardTo(Wrapping(3)));
}

#[test]
fn forward_to_init_query_forward_to_2_ticks() {
    forward_to_init_query(ProgressQuery::ForwardTo(Wrapping(4)));
}

#[test]
fn forward_to_init_query_forward_to_too_many_ticks() {
    forward_to_init_query(ProgressQuery::ForwardTo(Wrapping(5)));
}

#[test]
fn forward_to_init_query_forward_log() {
    forward_to_init_query(ProgressQuery::ForwardLog);
}

#[test]
fn forward_to_init_query_backward_log() {
    forward_to_init_query(ProgressQuery::BackwardLog);
}

#[test]
fn forward_to_init_query_log_to_past() {
    forward_to_init_query(ProgressQuery::LogTo(Wrapping(1)));
}

#[test]
fn forward_to_init_query_log_to_present() {
    forward_to_init_query(ProgressQuery::LogTo(Wrapping(2)));
}

#[test]
fn forward_to_init_query_log_to_future() {
    forward_to_init_query(ProgressQuery::LogTo(Wrapping(3)));
}

#[test]
fn forward_to_init_query_pause() {
    forward_to_init_query(ProgressQuery::Pause);
}

#[test]
fn forward_to_not_init_query_forward() {
    forward_to_not_init_query(ProgressQuery::Forward);
}

#[test]
fn forward_to_not_init_query_forward_to_present() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(3)));
}

#[test]
fn forward_to_not_init_query_forward_to_1_tick() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(4)));
}

#[test]
fn forward_to_not_init_query_forward_to_2_ticks() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(5)));
}

#[test]
fn forward_to_not_init_query_forward_to_too_many_ticks() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(6)));
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
    forward_to_not_init_query(ProgressQuery::LogTo(Wrapping(2)));
}

#[test]
fn forward_to_not_init_query_log_to_present() {
    forward_to_not_init_query(ProgressQuery::LogTo(Wrapping(3)));
}

#[test]
fn forward_to_not_init_query_log_to_future() {
    forward_to_not_init_query(ProgressQuery::LogTo(Wrapping(4)));
}

#[test]
fn forward_to_not_init_query_pause() {
    forward_to_not_init_query(ProgressQuery::Pause);
}
