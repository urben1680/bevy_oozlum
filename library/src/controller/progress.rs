use std::num::Wrapping;

use crate::Ticks;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Progress {
    Forward {
        after_forward: bool,
    },
    ForwardFast {
        after_forward_if_init: Option<bool>,
    },
    ForwardLog {
        after_forward: bool,
    },
    ForwardLogEnd {
        after_forward_if_init: Option<bool>,
    },
    BackwardLog {
        after_backward: bool,
    },
    BackwardLogEnd {
        after_backward_if_init: Option<bool>,
    },
    Pause {
        after_forward_if_log: Option<bool>,
    },
    LogClose {
        after_forward: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProgressQuery {
    Forward,
    ForwardFast { to_time_stamp: Wrapping<Ticks> },
    ForwardLog,
    ForwardLogEnd,
    BackwardLog,
    BackwardLogEnd,
    Pause,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum ProgressQueried {
    Forward,
    ForwardFast {
        to_time_stamp: Wrapping<Ticks>,
        queried: Wrapping<Ticks>,
    },
    ForwardLog,
    ForwardLogEnd,
    BackwardLog,
    BackwardLogEnd,
    Pause,
}

#[derive(PartialEq, Debug)]
pub(super) enum ProgressType {
    NotLog,
    ForwardLog,
    BackwardLog,
}

pub(super) enum Previous {
    Forward,
    Backward,
}
