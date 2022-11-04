use std::num::Wrapping;

use crate::controller::progress::{Progress, ProgressQuery, ProgressQueryError};

use super::{Command, RunTests, Test};

#[test]
fn backward_log_after_backward_query_none_to_end() {
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
            progress_query: Some(ProgressQuery::ForwardLog),
            commands: vec![Command::Redo],
        },
    ]
    .run(Ok(()));
}
