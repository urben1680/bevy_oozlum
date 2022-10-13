use crate::Ticks;

#[derive(PartialEq, Debug, Copy, Clone)]
pub(crate) struct ControllerConsts {
    pub(crate) max_log_index: Ticks,
    pub(crate) max_log_index_usize: usize,
    pub(crate) log_len: usize,
    pub(crate) delayed_commands_ticks_capacity: usize,
    pub(crate) delayed_commands_sync_sender_capacity: usize,
}

impl ControllerConsts {
    pub(crate) const fn new(
        max_log_index: Ticks,
        delayed_commands_ticks_capacity: usize,
        delayed_commands_sync_sender_capacity: usize,
    ) -> Self {
        Self {
            max_log_index,
            max_log_index_usize: max_log_index as usize,
            log_len: max_log_index as usize + 1,
            delayed_commands_ticks_capacity,
            delayed_commands_sync_sender_capacity,
        }
    }
    pub(crate) const fn max_log_index_only(max_log_index: Ticks) -> Self {
        Self::new(
            max_log_index,
            CONTROLLER_CONSTS.delayed_commands_ticks_capacity,
            CONTROLLER_CONSTS.delayed_commands_sync_sender_capacity,
        )
    }
}

pub(crate) const CONTROLLER_CONSTS: ControllerConsts =
    ControllerConsts::new(Ticks::MAX, Ticks::MAX as usize >> 1, 1024);
