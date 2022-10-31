use std::{num::Wrapping, ops::RangeInclusive};

use bevy::{ecs::system::Command, prelude::World};

use crate::Ticks;

use super::Controller;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(super) enum Progress {
    #[default]
    Forward,
    ForwardTo {
        init: bool,
    },
    ForwardLog {
        after_forward: bool,
    },
    ForwardLogTo {
        after_forward_if_init: Option<bool>,
    },
    BackwardLog {
        after_backward: bool,
    },
    BackwardLogTo {
        after_backward_if_init: Option<bool>,
    },
    LogClose {
        after_backward: bool,
    },
    Pause {
        after_forward_if_log: Option<bool>,
    },
}

#[derive(PartialEq, Debug)]
pub(super) enum TimeStampChange {
    NoChange,
    PlusOne,
    MinusOne,
}

impl Progress {
    pub(super) fn time_stamp_change(&self) -> TimeStampChange {
        match self {
            Progress::Forward
            | Progress::ForwardTo { .. }
            | Progress::ForwardLog {
                after_forward: true,
            }
            | Progress::ForwardLogTo {
                after_forward_if_init: None,
            }
            | Progress::ForwardLogTo {
                after_forward_if_init: Some(true),
            } => TimeStampChange::PlusOne,
            Progress::BackwardLog {
                after_backward: true,
            }
            | Progress::BackwardLogTo {
                after_backward_if_init: None,
            }
            | Progress::BackwardLogTo {
                after_backward_if_init: Some(true),
            }
            | Progress::LogClose {
                after_backward: true,
            } => TimeStampChange::MinusOne,
            _ => TimeStampChange::NoChange,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProgressQuery {
    Forward,
    ForwardTo(Wrapping<Ticks>),
    ForwardLog,
    BackwardLog,
    LogTo(Wrapping<Ticks>),
    Pause,
}

#[derive(PartialEq, Debug)]
pub(super) enum ProgressLog {
    NotLog,
    ForwardLog,
    BackwardLog,
}

impl Command for ProgressQuery {
    fn write(self, world: &mut World) {
        let mut controller = world.resource_mut::<Controller>();
        if let Err(err) = controller.query_progress(self) {
            #[cfg(debug_assertions)]
            {
                panic!(
                    "Invalid progress query: {self:?}, error: {err:?}, controller log: {:#?}",
                    controller.debug
                );
            }
            #[cfg(not(debug_assertions))]
            {
                panic!("Invalid progress query: {self}, error: {err}");
            }
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum ProgressQueryError {
    ToProgressActive,
    QueryOutOfRange(RangeInclusive<Wrapping<Ticks>>),
}

#[derive(Debug)]
pub enum QueryLimit {
    CurrentlyNotQueryable,
    CurrentLimit {
        forward_to_range: RangeInclusive<Wrapping<Ticks>>,
        log_to_range: RangeInclusive<Wrapping<Ticks>>,
    },
}
