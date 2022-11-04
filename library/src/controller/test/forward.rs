use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery};

use super::{Command, RunTests, Test};

#[test]
fn forward_query_none() {
    vec![
        Test {
            time_stamp: Some(1),
            forward_fast_range: 2..=3,
            log_range: 0..=1,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(2),
            forward_fast_range: 3..=4,
            log_range: 0..=2,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(3),
            forward_fast_range: 4..=5,
            log_range: 0..=3,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(4),
            forward_fast_range: 5..=6,
            log_range: 1..=4,
            progress_current: Progress::Forward,
            progress_query: None,
            commands: vec![Command::RedoFinalize, Command::Init],
        },
    ]
    .run(Ok(()));
}

/*
#[test]
fn forward_query_forward() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::Forward),
            commands: vec![Command::Init],
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
fn forward_query_forward_to_present() {
    [Test {
        time_stamp: Some(1),
        progress_current: Progress::Forward,
        progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(1))),
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
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(2))),
            commands: vec![Command::Init],
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
fn forward_query_forward_to_2_ticks() {
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
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_forward_to_too_many_ticks() {
    [Test {
        time_stamp: Some(1),
        progress_current: Progress::Forward,
        progress_query: Some(ProgressQuery::ForwardFastTo(Wrapping(4))),
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
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause(Some(true)),
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog(false),
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_backward_log() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog(false),
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_log_to_future() {
    [Test {
        time_stamp: Some(1),
        progress_current: Progress::Forward,
        progress_query: Some(ProgressQuery::LogFastTo(Wrapping(2))),
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
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::LogFastTo(Wrapping(1))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause(Some(true)),
            progress_query: Some(ProgressQuery::BackwardLog),
            commands: vec![],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLog(false),
            progress_query: None,
            commands: vec![Command::Undo],
        },
    ]
    .run(Ok(()));
}

#[test]
fn forward_query_log_to_past() {
    [
        Test {
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::LogFastTo(Wrapping(0))),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: Some(1),
            progress_current: Progress::BackwardLogFast(Some(false)),
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
            time_stamp: Some(1),
            progress_current: Progress::Forward,
            progress_query: Some(ProgressQuery::Pause),
            commands: vec![Command::Init],
        },
        Test {
            time_stamp: None,
            progress_current: Progress::Pause(None),
            progress_query: Some(ProgressQuery::Forward),
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
*/
