use bevy::prelude::World;

use crate::Ticks;

#[derive(PartialEq, Debug, Copy, Clone)]
pub(crate) struct ControllerConsts {
    pub(crate) max_log_index: Ticks,
    pub(crate) max_log_index_usize: usize,
    pub(crate) log_len: usize,
    pub(crate) commands_ticks_capacity: usize,
    pub(crate) sync_sender_capacity: usize,
    pub(crate) default_time_step: f64,
    pub(crate) debug_capacity: usize,
}

impl ControllerConsts {
    pub(crate) const fn new(
        max_log_index: Ticks,
        commands_ticks_capacity: usize,
        sync_sender_capacity: usize,
        default_time_step: f64,
        debug_capacity: usize,
    ) -> Self {
        Self {
            max_log_index,
            max_log_index_usize: max_log_index as usize,
            log_len: max_log_index as usize + 1,
            commands_ticks_capacity,
            sync_sender_capacity,
            default_time_step,
            debug_capacity,
        }
    }
    pub(crate) const fn max_log_index_only(max_log_index: Ticks) -> Self {
        Self::new(
            max_log_index,
            CONTROLLER_CONSTS.commands_ticks_capacity,
            CONTROLLER_CONSTS.sync_sender_capacity,
            CONTROLLER_CONSTS.default_time_step,
            CONTROLLER_CONSTS.debug_capacity,
        )
    }
    #[cfg(test)]
    pub(crate) fn get(world: &World) -> &Self {
        world.resource::<super::Controller>().consts()
    }
    #[cfg(not(test))]
    pub(crate) const fn get(_world: &World) -> &Self {
        &CONTROLLER_CONSTS
    }
}

pub(crate) const CONTROLLER_CONSTS: ControllerConsts =
    ControllerConsts::new(Ticks::MAX, Ticks::MAX as usize >> 1, 1024, 0.02, 64);
