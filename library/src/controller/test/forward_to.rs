use std::num::Wrapping;

use crate::controller::{
    debug::DebugLog,
    progress::{Progress, ProgressQueried, ProgressQuery},
};

use super::{tests, Test, CONTROLLER_CONSTS_TIME_STEP_ZERO};

const PROGRESS_FORWARD_TO_TO_3: [Option<ProgressQuery>; 1] =
    [Some(ProgressQuery::ForwardTo(Wrapping(3)))];

const STEP_2_AFTER_FIRST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::ForwardTo {
        after_forward_if_init: Some(true),
    },
    time_stamp: Wrapping(2),
    log_len: 2,
    time_step_query: None,
    time_step: 0.0,
    first_ran: true,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(3),
    log_index: 0,
    delayed_commands_len: 2,
    commands_overflows: 0,
};
const STEP_2_AFTER_LAST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::ForwardTo {
        after_forward_if_init: None,
    },
    delayed_commands_len: 1,
    ..STEP_2_AFTER_FIRST_CHECK
};
const STEP_3_AFTER_FIRST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::ForwardTo {
        after_forward_if_init: None,
    },
    time_stamp: Wrapping(3),
    log_len: 3,
    time_step_query: None,
    time_step: 0.0,
    first_ran: true,
    progress_query: None,
    forget: Wrapping(0),
    to_time_stamp: Wrapping(3),
    log_index: 0,
    delayed_commands_len: 1,
    commands_overflows: 0,
};
const STEP_3_AFTER_LAST_CHECK: DebugLog = DebugLog {
    progress_current: Progress::Forward {
        after_forward: true,
    },
    to_time_stamp: Wrapping(0),
    delayed_commands_len: 0,
    ..STEP_3_AFTER_FIRST_CHECK
};

#[test]
fn processes_none_query() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                after_first_check: DebugLog {
                    progress_query: None,
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: STEP_2_AFTER_LAST_CHECK,
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: STEP_3_AFTER_FIRST_CHECK,
                after_last_check: STEP_3_AFTER_LAST_CHECK,
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::Forward.into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::Forward),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Forward),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Forward),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_to_not_future() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(3)).into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(3),
                        queried: Wrapping(1),
                    }),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(3),
                        queried: Wrapping(1),
                    }),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(3),
                        queried: Wrapping(1),
                    }),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_to_one_tick() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(4)).into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(4),
                        queried: Wrapping(1),
                    }),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(4),
                        queried: Wrapping(1),
                    }),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(4),
                        queried: Wrapping(1),
                    }),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_to_two_ticks() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardTo(Wrapping(5)).into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(5),
                        queried: Wrapping(1),
                    }),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(5),
                        queried: Wrapping(1),
                    }),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardTo {
                        to_time_stamp: Wrapping(5),
                        queried: Wrapping(1),
                    }),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: None,
                    to_time_stamp: Wrapping(5),
                    delayed_commands_len: 2,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_forward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::ForwardLog.into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::ForwardLog),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardLog),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::ForwardLog),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::Pause {
                        after_forward_if_log: Some(true),
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_backward_log() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::BackwardLog.into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::BackwardLog),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::BackwardLog),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::BackwardLog),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::BackwardLog {
                        after_backward: false,
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_log_to_now() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::LogTo(Wrapping(3)).into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(3))),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(3))),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(3))),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::Pause {
                        after_forward_if_log: Some(true),
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_log_to_past() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::LogTo(Wrapping(2)).into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(2))),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(2))),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(2))),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::BackwardLogTo {
                        after_backward_if_init: Some(false),
                    },
                    progress_query: None,
                    to_time_stamp: Wrapping(2),
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
#[should_panic(
    expected = "`ProgressQueried::LogTo(Wrapping(4))` out of range of `Wrapping(0)..=Wrapping(3)`."
)]
fn processes_query_log_to_invalid() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::LogTo(Wrapping(4)).into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(4))),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(4))),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::LogTo(Wrapping(4))),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: STEP_3_AFTER_LAST_CHECK,
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_pause() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::Pause.into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::Pause),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Pause),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Pause),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::Pause {
                        after_forward_if_log: None,
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}

#[test]
fn processes_query_overwritten() {
    tests(
        CONTROLLER_CONSTS_TIME_STEP_ZERO,
        PROGRESS_FORWARD_TO_TO_3,
        [
            Test {
                //#1
                before_first_commands: vec![ProgressQuery::Pause.into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: Some(true),
                    },
                    progress_query: Some(ProgressQueried::Pause),
                    ..STEP_2_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Pause),
                    ..STEP_2_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
            Test {
                //#2
                before_first_commands: vec![ProgressQuery::Forward.into()],
                after_first_check: DebugLog {
                    progress_current: Progress::ForwardTo {
                        after_forward_if_init: None,
                    },
                    progress_query: Some(ProgressQueried::Forward),
                    ..STEP_3_AFTER_FIRST_CHECK
                },
                after_last_check: DebugLog {
                    progress_current: Progress::Forward {
                        after_forward: true,
                    },
                    progress_query: None,
                    ..STEP_3_AFTER_LAST_CHECK
                },
                ..Default::default()
            },
        ],
    )
}
