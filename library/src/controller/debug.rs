use std::num::Wrapping;

use crate::Ticks;

use super::{
    progress::{Progress, ProgressQuery},
    Controller,
};

#[derive(Debug)]
pub(super) struct DebugLogContainer {
    pub(super) after_last: Option<DebugLog>,
    pub(super) after_first: DebugLog,
}

#[derive(Debug, PartialEq)]
pub(super) struct DebugLog {
    pub(super) time_step_query: Option<f64>,
    pub(super) time_step: f64,
    pub(super) first_ran: bool,
    pub(super) progress_current: Progress,
    pub(super) progress_query: Option<ProgressQuery>,
    pub(super) time_stamp: Wrapping<Ticks>,
    pub(super) forget: Wrapping<Ticks>,
    pub(super) to_time_stamp: Wrapping<Ticks>,
    pub(super) log_len: usize,
    pub(super) log_at_index_len: usize,
    pub(super) log_index: usize,
    pub(super) delayed_commands_len: usize,
    pub(super) commands_overflows: u64,
}

impl From<&Controller> for DebugLog {
    fn from(value: &Controller) -> Self {
        Self {
            time_step_query: value.time_step_query,
            time_step: value.time_step,
            first_ran: value.first_ran,
            progress_current: value.progress_current,
            progress_query: value.progress_query,
            time_stamp: value.time_stamp,
            forget: value.forget,
            to_time_stamp: value.to_time_stamp.to_time_stamp,
            log_len: value.log.len(),
            log_at_index_len: value.log.get(value.log_index).expect("log_index out of range").len(),
            log_index: value.log_index,
            delayed_commands_len: value.delayed_commands.len(),
            commands_overflows: value.commands_overflows,
        }
    }
}
