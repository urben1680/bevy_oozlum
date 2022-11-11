use crate::controller::progress::Progress;

use super::{Command, Query, Test};

#[test]
fn query_at_init() {
    let setup = vec![
        Test {
            time_stamp: Some(1),
            forward_fast_range: 2..=4,
            log_range: 0..=1,
            progress_current: Progress::Forward,
            commands: vec![Command::Init],
            query: Query::None,
        },
        Test {
            time_stamp: Some(2),
            forward_fast_range: 3..=5,
            log_range: 0..=2,
            progress_current: Progress::Forward,
            commands: vec![Command::Init],
            query: Query::None,
        },
    ];
    let branches = ([
        (
            Query::None,
            vec![Test {
                time_stamp: Some(3),
                forward_fast_range: 4..=6,
                log_range: 0..=3,
                progress_current: Progress::Forward,
                commands: vec![Command::Init],
                query: Query::None,
            }],
        ),
        (
            Query::Forward,
            vec![Test {
                time_stamp: Some(3),
                forward_fast_range: 4..=6,
                log_range: 0..=3,
                progress_current: Progress::Forward,
                commands: vec![Command::Init],
                query: Query::None,
            }],
        ),
        (
            Query::ForwardFastToRangeStart,
            vec![Test {
                time_stamp: Some(3),
                forward_fast_range: 4..=6,
                log_range: 0..=3,
                progress_current: Progress::Forward,
                commands: vec![Command::Init],
                query: Query::None,
            }],
        ),
        (
            Query::ForwardFastToRangeEnd,
            vec![Test {
                time_stamp: Some(3),
                forward_fast_range: 6..=8,
                log_range: 2..=5,
                progress_current: Progress::ForwardFast(true),
                commands: vec![],
                query: Query::None,
            }],
        ),
        (
            Query::LogToRangeStart,
            vec![
                Test {
                    time_stamp: Some(2),
                    forward_fast_range: 2..=4,
                    log_range: 0..=2,
                    progress_current: Progress::BackwardLog(false),
                    commands: vec![Command::Undo],
                    query: Query::None,
                },
                Test {
                    time_stamp: Some(1),
                    forward_fast_range: 1..=3,
                    log_range: 0..=2,
                    progress_current: Progress::BackwardLog(true),
                    commands: vec![Command::Undo],
                    query: Query::None,
                },
                Test {
                    time_stamp: None,
                    forward_fast_range: 1..=3,
                    log_range: 0..=2,
                    progress_current: Progress::Pause(Some(false)),
                    commands: vec![],
                    query: Query::None,
                },
            ],
        ),
        (
            Query::LogToRangeEnd,
            vec![Test {
                time_stamp: None,
                forward_fast_range: 3..=5,
                log_range: 0..=2,
                progress_current: Progress::Pause(Some(true)),
                commands: vec![],
                query: Query::None,
            }],
        ),
        (
            Query::LogFastToRangeStart,
            vec![
                Test {
                    time_stamp: Some(2),
                    forward_fast_range: 1..=3,
                    log_range: 0..=2,
                    progress_current: Progress::BackwardLogFast(Some(false)),
                    commands: vec![Command::Undo],
                    query: Query::None,
                },
                Test {
                    time_stamp: Some(1),
                    forward_fast_range: 1..=3,
                    log_range: 0..=2,
                    progress_current: Progress::BackwardLogFast(None),
                    commands: vec![Command::Undo],
                    query: Query::None,
                },
                Test {
                    time_stamp: None,
                    forward_fast_range: 1..=3,
                    log_range: 0..=2,
                    progress_current: Progress::Pause(Some(false)),
                    commands: vec![],
                    query: Query::None,
                },
            ],
        ),
        (
            Query::LogFastToRangeEnd,
            vec![Test {
                time_stamp: None,
                forward_fast_range: 3..=4,
                log_range: 0..=2,
                progress_current: Progress::Pause(Some(true)),
                commands: vec![],
                query: Query::None,
            }],
        ),
        (
            Query::Pause,
            vec![Test {
                time_stamp: None,
                forward_fast_range: 3..=4,
                log_range: 0..=2,
                progress_current: Progress::Pause(None),
                commands: vec![],
                query: Query::None,
            }],
        ),
    ])
    .into();
    Test::test_all_queries(setup, branches);
}
