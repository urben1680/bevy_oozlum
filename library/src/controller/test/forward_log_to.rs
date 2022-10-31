use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

fn forward_to_init_after_forward_query(query: ProgressQuery) {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::BackwardLog {
                after_backward: true,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::LogTo(Wrapping(2))),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::ForwardLogTo {
                after_forward_if_init: Some(true),
            },
            progress_query: Some(query),
            commands: vec![Command::Redo],
        },
    ]
    .run(Err(ProgressQueryError::ToProgressActive))
}

fn forward_to_init_after_backward_query(query: ProgressQuery) {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::BackwardLog {
                after_backward: true,
            },
            progress_query: Some(ProgressQuery::LogTo(Wrapping(2))),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::ForwardLogTo {
                after_forward_if_init: Some(false),
            },
            progress_query: Some(query),
            commands: vec![Command::Redo],
        },
    ]
    .run(Err(ProgressQueryError::ToProgressActive))
}

fn forward_to_not_init_query(query: ProgressQuery) {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::BackwardLog {
                after_backward: true,
            },
            progress_query: Some(ProgressQuery::LogTo(Wrapping(2))),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::ForwardLogTo {
                after_forward_if_init: Some(false),
            },
            progress_query: None,
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::ForwardLogTo {
                after_forward_if_init: None,
            },
            progress_query: Some(query),
            commands: vec![Command::Redo],
        },
    ]
    .run(Err(ProgressQueryError::ToProgressActive))
}

#[test]
fn forward_to_init_after_forward_query_none() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::BackwardLog {
                after_backward: false,
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::BackwardLog {
                after_backward: true,
            },
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Undo],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::ForwardLog {
                after_forward: false,
            },
            progress_query: Some(ProgressQuery::LogTo(Wrapping(2))),
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::ForwardLogTo {
                after_forward_if_init: Some(true),
            },
            progress_query: None,
            commands: vec![Command::Redo],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::Pause {
                after_forward_if_log: Some(true),
            },
            progress_query: None,
            commands: vec![],
        },
    ]
    .run(Ok(()))
}

#[test]
fn forward_to_init_after_forward_query_forward() {
    forward_to_init_after_forward_query(ProgressQuery::Forward);
}

#[test]
fn forward_to_init_after_forward_query_forward_to_present() {
    forward_to_init_after_forward_query(ProgressQuery::ForwardTo(Wrapping(2)));
}

#[test]
fn forward_to_init_after_forward_query_forward_to_1_tick() {
    forward_to_init_after_forward_query(ProgressQuery::ForwardTo(Wrapping(3)));
}

#[test]
fn forward_to_init_after_forward_query_forward_to_2_ticks() {
    forward_to_init_after_forward_query(ProgressQuery::ForwardTo(Wrapping(4)));
}

#[test]
fn forward_to_init_after_forward_query_forward_to_too_many_ticks() {
    forward_to_init_after_forward_query(ProgressQuery::ForwardTo(Wrapping(5)));
}

#[test]
fn forward_to_init_after_forward_query_forward_log() {
    forward_to_init_after_forward_query(ProgressQuery::ForwardLog);
}

#[test]
fn forward_to_init_after_forward_query_backward_log() {
    forward_to_init_after_forward_query(ProgressQuery::BackwardLog);
}

#[test]
fn forward_to_init_after_forward_query_log_to_past() {
    forward_to_init_after_forward_query(ProgressQuery::LogTo(Wrapping(1)));
}

#[test]
fn forward_to_init_after_forward_query_log_to_present() {
    forward_to_init_after_forward_query(ProgressQuery::LogTo(Wrapping(2)));
}

#[test]
fn forward_to_init_after_forward_query_log_to_future() {
    forward_to_init_after_forward_query(ProgressQuery::LogTo(Wrapping(3)));
}

#[test]
fn forward_to_init_after_forward_query_log_pause() {
    forward_to_init_after_forward_query(ProgressQuery::Pause);
}

#[test]
fn forward_to_init_after_backward_query_log_forward() {
    forward_to_init_after_backward_query(ProgressQuery::Forward);
}

#[test]
fn forward_to_init_after_backward_query_forward_to_present() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardTo(Wrapping(2)));
}

#[test]
fn forward_to_init_after_backward_query_forward_to_1_tick() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardTo(Wrapping(3)));
}

#[test]
fn forward_to_init_after_backward_query_forward_to_2_ticks() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardTo(Wrapping(4)));
}

#[test]
fn forward_to_init_after_backward_query_forward_to_too_many_ticks() {
    forward_to_init_after_backward_query(ProgressQuery::ForwardTo(Wrapping(5)));
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
    forward_to_init_after_backward_query(ProgressQuery::LogTo(Wrapping(1)));
}

#[test]
fn forward_to_init_after_backward_query_log_to_present() {
    forward_to_init_after_backward_query(ProgressQuery::LogTo(Wrapping(2)));
}

#[test]
fn forward_to_init_after_backward_query_log_to_future() {
    forward_to_init_after_backward_query(ProgressQuery::LogTo(Wrapping(3)));
}

#[test]
fn forward_to_init_after_backward_query_log_pause() {
    forward_to_init_after_backward_query(ProgressQuery::Pause);
}

#[test]
fn forward_to_not_init_query_forward() {
    forward_to_not_init_query(ProgressQuery::Forward);
}

#[test]
fn forward_to_not_init_query_forward_to_present() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(2)));
}

#[test]
fn forward_to_not_init_query_forward_to_1_tick() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(3)));
}

#[test]
fn forward_to_not_init_query_forward_to_2_ticks() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(4)));
}

#[test]
fn forward_to_not_init_query_forward_to_too_many_ticks() {
    forward_to_not_init_query(ProgressQuery::ForwardTo(Wrapping(5)));
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
    forward_to_not_init_query(ProgressQuery::LogTo(Wrapping(1)));
}

#[test]
fn forward_to_not_init_query_log_to_present() {
    forward_to_not_init_query(ProgressQuery::LogTo(Wrapping(2)));
}

#[test]
fn forward_to_not_init_query_log_to_future() {
    forward_to_not_init_query(ProgressQuery::LogTo(Wrapping(3)));
}

#[test]
fn forward_to_not_init_query_log_pause() {
    forward_to_not_init_query(ProgressQuery::Pause);
}
