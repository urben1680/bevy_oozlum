use std::num::Wrapping;

use crate::Ticks;

/// `Progress` is used to control the progression of all reversible systems.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Progress {
    #[default]
    /// `Forward` progresses all systems step-by-step in sync.
    Forward,
    /// `ForwardFast { to_time_stamp }` progresses some systems eagerly until `to_time_stamp` is reached.
    ///
    /// This cannot be aborted because affected systems are not in sync until the end.
    /// However, it can be extended at any point by setting this variant again with a larger `to_time_stamp` relatively to `time_stamp()`.
    ///
    /// If `to_time_stamp` was reached and still `FastForward` is queried, the progression is changed to `Forward`.
    ForwardFast { to_time_stamp: Wrapping<Ticks> },
    /// `ForwardLog` progresses all systems using each system's log(s) step-by-step.
    /// If this is attempted while the log reached it's most recent end, the progression is changed to `PauseLog`.
    ForwardLog,
    /// `ForwardLogEnd` progresses to the most recent log end, potentially cheaper than with `ForwardLog`.
    /// This cannot be aborted because affected systems are not in sync until the end.
    ForwardLogEnd,
    /// `BackwardLog` reverts all systems using each system's log(s) step-by-step.
    BackwardLog,
    /// `BackwardLogEnd` reverts all systems to the most past log end, potentially cheaper than with `BackwardLog`.
    /// This cannot be aborted because affected systems are not in sync until the end.
    BackwardLogEnd,
    /// `Pause` halts everything until another non-pause variant is picked.
    Pause,
    /// `PauseLog` behaves like `Pause` but does not cause logs in the future to be forgotten.
    PauseLog,
}

impl Progress {
    /// Returns `true` if `self` is `ForwardLog`, `ForwardLogEnd`, `BackwardLog` or `BackwardLogEnd`.
    ///
    /// If parameter `including_pause` is set to true, the above list includes `PauseLog`.
    ///
    /// Otherwise returns `false`.
    pub fn is_log(&self, including_pause: bool) -> bool {
        matches!(
            self,
            Progress::ForwardLog
                | Progress::ForwardLogEnd
                | Progress::BackwardLog
                | Progress::BackwardLogEnd
        ) || (including_pause && self == &Progress::PauseLog)
    }
    /// Returns `true` if `self` is `Pause` or `PauseLog`.
    ///
    /// Otherwise returns `false`.
    pub fn is_pause(&self) -> bool {
        matches!(self, Progress::Pause | Progress::PauseLog)
    }

    /// Returns `true` if `self` is `Forward`, `ForwardLog` or `BackwardLog`.
    /// In `Controller` this causes progression in fixed time steps.
    ///
    /// Otherwise returns `false`. In `Controller` this causes progression as fast as possible.
    pub fn is_fixed_time_step(&self) -> bool {
        matches!(
            self,
            Progress::Forward | Progress::ForwardLog | Progress::BackwardLog
        )
    }
    /// Returns `true` if `self` is `Forward` or `ForwardFast`.
    ///
    /// Otherwise returns `false`.
    pub fn is_not_log_nor_pause(&self) -> bool {
        matches!(self, Progress::Forward | Progress::ForwardFast { .. })
    }
    /// Returns `true` if `self` is `ForwardFast`, `ForwardLogEnd` of `BackwardLogEnd`.
    ///
    /// Otherwise returns `false`.
    pub fn is_fast(&self) -> bool {
        matches!(
            self,
            Progress::ForwardFast { .. } | Progress::ForwardLogEnd | Progress::BackwardLogEnd
        )
    }
}
