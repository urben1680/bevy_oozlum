use bevy::prelude::World;

use crate::Ticks;

#[derive(PartialEq, Debug, Copy, Clone)]
pub(crate) struct ControllerConsts {
    pub(crate) max_log_index: Ticks,
    pub(crate) max_log_index_usize: usize,
    pub(crate) log_capacity: usize,
    pub(crate) forward_to_max: Ticks,
    pub(crate) delayed_commands_capacity: usize,
    pub(crate) sync_sender_capacity: usize,
    pub(crate) default_time_step: f64,
    pub(crate) debug_capacity: usize,
}

impl ControllerConsts {
    pub(crate) const fn new(
        max_log_index: Ticks,
        forward_to_max: Ticks,
        sync_sender_capacity: usize,
        default_time_step: f64,
        debug_capacity: usize,
    ) -> Self {
        if max_log_index == Ticks::MAX {
            panic!("`max_log_index` should not be equal `Ticks::MAX` because log indices are off by one to adress pre_log meta. See `log_system::Log::entry`");
        }
        if forward_to_max == 0 {
            panic!("`fast_forward_max` should not be 0");
        }
        Self {
            max_log_index,
            max_log_index_usize: max_log_index as usize,
            log_capacity: max_log_index as usize + 1,
            forward_to_max,
            delayed_commands_capacity: forward_to_max as usize + 1,
            sync_sender_capacity,
            default_time_step,
            debug_capacity,
        }
    }
    pub(crate) const fn max_log_index_only(max_log_index: Ticks) -> Self {
        Self::new(
            max_log_index,
            CONTROLLER_CONSTS.forward_to_max,
            CONTROLLER_CONSTS.sync_sender_capacity,
            CONTROLLER_CONSTS.default_time_step,
            CONTROLLER_CONSTS.debug_capacity,
        )
    }
}

pub(crate) const CONTROLLER_CONSTS: ControllerConsts =
    ControllerConsts::new(Ticks::MAX - 1, Ticks::MAX >> 1, 1024, 0.02, 64);
