use std::{num::Wrapping, ops::RangeInclusive};

use bevy::{ecs::system::Command, prelude::World};

use crate::Ticks;

use super::Controller;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub(super) enum Progress {
    #[default]
    Forward,
    /// Inner value: init`
    ForwardFast(bool),
    /// Inner value: after_forward
    ForwardLog(bool),
    /// Inner value: after_forward_if_init
    ForwardLogFast(Option<bool>),
    /// Inner value: after_backward
    BackwardLog(bool),
    /// Inner value: after_backward_if_init
    BackwardLogFast(Option<bool>),
    /// Inner value: after_backward
    LogClose(bool),
    /// Inner value: after_forward_if_log
    Pause(Option<bool>),
}

enum After {
    AfterForward,
    AfterBackward,
}
enum AfterIfInit {
    InitAfterForward,
    InitAfterBackward,
    NotInit,
}
enum AfterIfLog {
    LogAfterForward,
    LogAfterBackward,
    NotLog,
}
enum Init {
    Init,
    NotInit,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProgressQuery {
    Forward,
    ForwardFastTo(Wrapping<Ticks>), //changes to Forward at time_stamp
    LogTo(Wrapping<Ticks>),         //changes to Pause at time_stamp
    LogFastTo(Wrapping<Ticks>),     //changes to Pause at time_stamp
    Pause,
}

#[derive(PartialEq, Debug)]
pub(super) enum ProgressLog {
    NotLog,
    ForwardLog,
    BackwardLog,
}

impl ProgressLog {
    pub(super) fn after_forward(&self) -> bool {
        match self {
            ProgressLog::NotLog => true,
            ProgressLog::ForwardLog => true,
            ProgressLog::BackwardLog => false,
        }
    }
    pub(super) fn after_backward(&self) -> bool {
        !self.after_forward()
    }
    pub(super) fn after_forward_if_log(&self) -> Option<bool> {
        match self {
            ProgressLog::NotLog => None,
            ProgressLog::ForwardLog => Some(true),
            ProgressLog::BackwardLog => Some(false),
        }
    }
    pub(super) fn after_backward_if_log(&self) -> Option<bool> {
        self.after_forward_if_log().map(|b| !b)
    }
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
