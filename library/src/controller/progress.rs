use std::{num::Wrapping, ops::RangeInclusive};

use bevy::{ecs::system::Command, prelude::World};

use crate::Ticks;

use super::Controller;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Progress {
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
        /*
        - log forward then close: no changes to log index or stamps
        - log backward then close: forward would not call stamps_forward. But to simplify non-log progress, now stamps should be altered so forward can carelessly call stamps_forward
        - log pause: same as log backward if after_forward_if_log = Some(false)

        stamps_forward:
            self.time_stamp += 1;
            self.forget += 1;
        */
        after_backward: bool,
    },
    Pause {
        after_forward_if_log: Option<bool>,
    },
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
        if let Err(err) = controller.query_progress(self){
            #[cfg(debug_assertions)]
            {
                panic!("Invalid progress query: {self:?}, error: {err:?}, controller log: {:#?}", controller.debug);
            }
            #[cfg(not(debug_assertions))]
            {
                panic!("Invalid progress query: {self}, error: {err}");
            }
        }
    }
}

#[derive(Debug)]
pub enum ProgressQueryError{
    ForwardToOrLogTo,
    QueryFortwardToPresent,
    QueryOutOfRange(RangeInclusive<Wrapping<Ticks>>)
}

pub enum QueryLimit{
    CurrentlyNotQueryable,
    CurrentLimit {
        forward_to_panic: Wrapping<Ticks>,
        log_to_range: RangeInclusive<Wrapping<Ticks>>
    }
}