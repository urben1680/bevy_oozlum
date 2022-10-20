use std::{
    collections::VecDeque,
    num::Wrapping,
    sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError},
};

use bevy::{
    prelude::{info, Commands, Mut, NonSendMut, World},
    time::Time,
};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks, TicksRelative,
};

use super::consts::ControllerConsts;

/// `NonSend` resource containing sync channel `Receiver`s for forgets and delayed commands.
pub(super) struct ControllerReceivers {
    /// Messages about commands that are not happening in the next tick, is only sent to if progress is `ForwardFast`.
    pub(super) commands: Receiver<(usize, Vec<Box<dyn ReversibleCommand>>)>,
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
pub enum ProgressQueried {
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

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum CurrentProgress {
    #[default]
    Forward,
    ForwardFast {
        init: bool,
    },
    ForwardLog {
        after_backward: bool,
    },
    ForwardLogEnd {
        after_backward_if_init: Option<bool>,
    },
    BackwardLog {
        after_forward: bool,
    },
    BackwardLogEnd {
        after_forward_if_init: Option<bool>,
    },
    Pause {
        after_forward_if_log: Option<bool>,
    },
    LogClose {
        after_forward: bool,
    },
}

pub(super) enum ProgressType {
    NotLog,
    ForwardLog,
    BackwardLog,
}