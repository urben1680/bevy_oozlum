use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

#[test]
fn forward_query_none() {
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
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 3,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 4,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::RedoFinalize, Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_forward() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::Forward),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_forward_to_present() {
    [Test {
        time_stamp: 1,
        progress_current: Progress::Forward,
        progress_query: Some(ProgressQuery::ForwardTo(Wrapping(1))),
        commands: vec![Command::Init],
    }]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(2)..=Wrapping(3),
    )));
}

#[test]
fn forward_query_forward_to_1_tick() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardTo(Wrapping(2))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 2,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_forward_to_2_ticks() {
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
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_forward_to_too_many_ticks() {
    [Test {
        time_stamp: 1,
        progress_current: Progress::Forward,
        progress_query: Some(ProgressQuery::ForwardTo(Wrapping(4))),
        commands: vec![Command::Init],
    }]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(2)..=Wrapping(3),
    )));
}

#[test]
fn forward_query_forward_log() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::Pause {
                after_forward_if_log: Some(true),
            },
            progress_query: None,
            commands: vec![],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_backward_log() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 1,
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
fn forward_query_log_to_future() {
    [Test {
        time_stamp: 1,
        progress_current: Progress::Forward,
        progress_query: Some(ProgressQuery::LogTo(Wrapping(2))),
        commands: vec![Command::Init],
    }]
    .run(Err(ProgressQueryError::QueryOutOfRange(
        Wrapping(0)..=Wrapping(1),
    )));
}

#[test]
fn forward_query_log_to_present() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::LogTo(Wrapping(1))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::Pause {
                after_forward_if_log: Some(true),
            },
            progress_query: None,
            commands: vec![],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_log_to_past() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::LogTo(Wrapping(0))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::BackwardLogTo {
                after_backward_if_init: Some(false),
            },
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_pause() {
    [
        Test {
            time_stamp: 1,
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::Pause),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: 1,
            progress_current: Progress::Pause {
                after_forward_if_log: None,
            },
            progress_query: None,
            commands: vec![],
        },
    ]
    .run(Ok(()));
}
