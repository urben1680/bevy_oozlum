use std::{
    collections::VecDeque,
    iter::FromIterator,
    num::Wrapping,
    sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError},
};

use bevy::{
    ecs::world::Mut,
    log::info,
    prelude::{Commands, NonSendMut, World},
    time::Time,
};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks, TicksRelative,
};

use self::{
    consts::ControllerConsts,
    debug::DebugLogContainer,
    progress::{Progress, ProgressQueried, ProgressQuery, ProgressType},
};

pub(crate) mod consts;
pub(crate) mod progress;

#[cfg(debug_assertions)]
mod debug;

//#[cfg(test)]
//mod test;

/// `NonSend` resource containing sync channel `Receiver`s for forgets and delayed commands.
pub(super) struct ControllerReceiver(Receiver<(usize, Vec<Box<dyn ReversibleCommand>>)>);

pub(super) struct Controller {
    time_step_query: Option<f64>, //todo: use enum instead with options such as "back to default after n ticks"
    time_step: f64,
    elapsed: f64,
    time: Time,
    first_ran: bool,

    current: Progress,
    query: Option<ProgressQueried>,
    time_stamp: Wrapping<Ticks>,
    forget: Wrapping<Ticks>,
    forward_fast_to: Wrapping<Ticks>,

    log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
    log_index: usize,
    delayed_commands: VecDeque<Vec<Box<dyn ReversibleCommand>>>,
    commands_overflows: u64,
    commands_sender: SyncSender<(usize, Vec<Box<dyn ReversibleCommand>>)>,

    #[cfg(debug_assertions)]
    debug: VecDeque<DebugLogContainer>,
    #[cfg(test)]
    constants: ControllerConsts,
}

impl Controller {
    #[cfg(test)]
    pub(super) fn consts(&self) -> &ControllerConsts {
        &self.constants
    }
    #[cfg(not(test))]
    pub(super) const fn consts(&self) -> &ControllerConsts {
        &self::consts::CONTROLLER_CONSTS
    }
    pub(super) fn time_stamp(&self) -> Wrapping<Ticks> {
        self.time_stamp
    }
    pub(super) fn forget(&self) -> Wrapping<Ticks> {
        self.forget
    }
    pub(super) fn forward_fast_to(&self) -> Wrapping<Ticks> {
        self.forward_fast_to
    }
    pub(super) fn log_start(&self) -> Wrapping<Ticks> {
        self.time_stamp - Wrapping(self.log.len() as Ticks)
    }
    pub(super) fn send_commands(
        &self,
        v: Vec<Box<dyn ReversibleCommand>>,
        commands: &mut Commands<'_, '_>,
        from_now: Ticks,
    ) {
        match self.commands_sender.try_send((from_now as usize, v)) {
            Ok(_) => {}
            Err(TrySendError::Full((index, command))) => commands.add(move |world: &mut World| {
                let mut controller = world.resource_mut::<Self>();
                controller.commands_overflows += 1;
                controller.add_delayed_commands(index, command);
            }),
            Err(TrySendError::Disconnected(_)) => {
                panic!("Could not send delayed command, receiver disconnected.");
            }
        }
    }
    pub(super) fn into_world(
        time_stamp: Wrapping<Ticks>,
        log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
        after_forward: bool,
        constants: ControllerConsts,
        world: &mut World,
    ) {
        let (commands_sender, commands_receiver) = sync_channel(constants.sync_sender_capacity);
        world.insert_non_send_resource(ControllerReceiver(commands_receiver));
        world.insert_resource(Self {
            time_step_query: None,
            time_step: constants.default_time_step,
            elapsed: 0.0,
            time: Default::default(),
            first_ran: false,
            current: Progress::Forward { after_forward },
            query: None,
            time_stamp,
            forget: time_stamp - Wrapping(constants.max_log_index),
            forward_fast_to: Default::default(),
            log,
            log_index: 0,
            delayed_commands: VecDeque::with_capacity(constants.commands_ticks_capacity),
            commands_overflows: 0,
            commands_sender,

            #[cfg(debug_assertions)]
            debug: VecDeque::with_capacity(constants.debug_capacity),

            #[cfg(test)]
            constants,
        });
    }
    pub(super) fn process_first(&mut self, commands: &mut Commands<'_, '_>) {
        match self.current {
            Progress::Forward { after_forward } => {
                if self.first_time() {
                    self.first_forward_not_log(commands);
                    self.stamps_indices_forward(after_forward, false);
                    self.first_ran = true;
                }
            }
            Progress::ForwardFast {
                after_forward_if_init,
            } => {
                self.time.update();
                self.first_forward_not_log(commands);
                self.stamps_indices_forward(after_forward_if_init == Some(true), false);
                self.first_ran = true;
            }
            Progress::ForwardLog { after_forward } => {
                if self.first_time() {
                    self.stamps_indices_forward(after_forward, true);
                    self.first_ran = true;
                }
            }
            Progress::ForwardLogEnd {
                after_forward_if_init,
            } => {
                self.stamps_indices_forward(after_forward_if_init == Some(true), false);
                self.first_ran = true;
            }
            Progress::BackwardLog { after_backward } => {
                if self.first_time() {
                    self.stamps_indices_backward(after_backward);
                    self.first_backward(commands);
                    self.first_ran = true;
                }
            }
            Progress::BackwardLogEnd {
                after_backward_if_init,
            } => {
                self.stamps_indices_backward(after_backward_if_init == Some(true));
                self.first_backward(commands);
                self.first_ran = true;
            }
            Progress::Pause { .. } => {
                self.time.update();
                self.first_ran = true;
            }
            Progress::LogClose { .. } => {
                self.first_ran = true;
            }
        }
        #[cfg(debug_assertions)]
        self.update_debug(true);
    }
    pub(super) fn process_last(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceiver>,
        commands: Commands<'_, '_>,
    ) {
        self.first_ran = false;
        self.time_step = self.time_step_query.take().unwrap_or(self.time_step);
        match self.current {
            Progress::Forward { .. } => {
                self.last_forward_commands(receivers, commands, false);
                self.current = Progress::Forward {
                    after_forward: false,
                };
                self.apply_progress_query(ProgressType::NotLog);
            }
            Progress::ForwardFast { .. } => {
                self.last_forward_commands(receivers, commands, true);
                if self.forward_fast_to == self.time_stamp {
                    self.current = Progress::Pause {
                        after_forward_if_log: None,
                    };
                    self.apply_progress_query(ProgressType::NotLog);
                } else {
                    self.current = Progress::ForwardFast {
                        after_forward_if_init: None,
                    };
                }
            }
            Progress::ForwardLog { .. } => {
                self.last_forward_commands_log(commands);
                self.current = match self.log_index_min() {
                    true => Progress::Pause {
                        after_forward_if_log: Some(true),
                    },
                    false => Progress::ForwardLog {
                        after_forward: true,
                    },
                };
                self.apply_progress_query(ProgressType::ForwardLog);
            }
            Progress::ForwardLogEnd { .. } => {
                self.last_forward_commands_log(commands);
                self.current = Progress::Pause {
                    after_forward_if_log: Some(true),
                };
                if self.log_index_min() {
                    self.apply_progress_query(ProgressType::ForwardLog);
                }
            }
            Progress::BackwardLog { .. } => {
                self.current = match self.log_index_max() {
                    true => Progress::Pause {
                        after_forward_if_log: Some(false),
                    },
                    false => Progress::BackwardLog {
                        after_backward: true,
                    },
                };
                self.apply_progress_query(ProgressType::BackwardLog);
            }
            Progress::BackwardLogEnd { .. } => {
                if self.log_index_max() {
                    self.current = Progress::Pause {
                        after_forward_if_log: Some(false),
                    };
                    self.apply_progress_query(ProgressType::BackwardLog);
                } else {
                    self.current = Progress::BackwardLogEnd {
                        after_backward_if_init: None,
                    };
                }
            }
            Progress::Pause {
                after_forward_if_log,
            } => match after_forward_if_log {
                None => self.apply_progress_query(ProgressType::NotLog),
                Some(false) => self.apply_progress_query(ProgressType::BackwardLog),
                Some(true) => self.apply_progress_query(ProgressType::ForwardLog),
            },
            Progress::LogClose { after_forward } => {
                self.log_close_split(commands, after_forward);
                self.current = Progress::Forward { after_forward };
                self.apply_progress_query(ProgressType::NotLog)
            }
        }
        #[cfg(debug_assertions)]
        self.update_debug(false);
    }
    fn stamps_indices_forward(&mut self, after_forward: bool, log: bool) {
        if !after_forward {
            return;
        }
        self.time_stamp += 1;
        self.forget += 1;
        if log {
            assert_ne!(self.log_index, 0);
            self.log_index -= 1;
        }
    }
    fn stamps_indices_backward(&mut self, after_backward: bool) {
        if !after_backward {
            return;
        }
        self.time_stamp -= 1;
        self.forget -= 1;
        self.log_index += 1;
        assert_ne!(self.log_index, self.log.len());
    }
    fn query(&mut self, query: ProgressQuery) {
        self.query = Some(match query {
            ProgressQuery::Forward => ProgressQueried::Forward,
            ProgressQuery::ForwardFast { to_time_stamp } => ProgressQueried::ForwardFast {
                to_time_stamp,
                queried: self.time_stamp,
            },
            ProgressQuery::ForwardLog => ProgressQueried::ForwardLog,
            ProgressQuery::ForwardLogEnd => ProgressQueried::ForwardLogEnd,
            ProgressQuery::BackwardLog => ProgressQueried::BackwardLog,
            ProgressQuery::BackwardLogEnd => ProgressQueried::BackwardLogEnd,
            ProgressQuery::Pause => ProgressQueried::Pause,
        });
    }
    fn first_forward_not_log(&mut self, commands: &mut Commands<'_, '_>) {
        if self.log.len() == self.consts().log_len {
            let to_finalize = self.log.pop_back().expect("`MAX_LOG_LEN` should not be 0.");
            if !to_finalize.is_empty() {
                commands.add(|world: &mut World| {
                    to_finalize
                        .into_iter()
                        .for_each(|command| command.redo_finalize(world))
                });
            }
        }
        self.log.push_front(Default::default());
    }
    fn first_backward(&mut self, commands: &mut Commands<'_, '_>) {
        self.time_stamp -= 1;
        self.forget -= 1;
        if let Some(v) = self.log.get(self.log_index) {
            if v.is_empty() {
                panic!("todo");
            }
            commands.add(|world: &mut World| {
                world.resource_scope(|world, mut controller: Mut<'_, Self>| {
                    let index = controller.log_index;
                    if let Some(entry) = controller.log.get_mut(index) {
                        entry.iter_mut().for_each(|command| command.undo(world));
                    } else {
                        panic!("todo");
                    }
                });
            });
        }
    }
    fn first_time(&mut self) -> bool {
        self.elapsed -= self.time.delta_seconds_f64();
        self.time.update();
        if self.elapsed > 0.0 {
            return false;
        }
        self.elapsed += self.time_step;
        true
    }
    fn add_delayed_commands(
        &mut self,
        index: usize,
        mut commands: Vec<Box<dyn ReversibleCommand>>,
    ) {
        if let Some(v) = self.delayed_commands.get_mut(index) {
            v.append(&mut commands);
        } else {
            panic!("todo");
        }
    }
    fn last_forward_commands(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceiver>,
        mut commands: Commands<'_, '_>,
        fast: bool,
    ) {
        if fast {
            receivers
                .0
                .try_iter()
                .for_each(|(index, command)| self.add_delayed_commands(index, command));
            if self.commands_overflows != 0 {
                info!(
                    "`delayed_commands_overflows` {} with DELAYED_COMMANDS_SYNC_SENDER_CAPACITY {}",
                    self.commands_overflows,
                    self.consts().sync_sender_capacity
                );
                self.commands_overflows = 0;
            }
        } else {
            assert!(
                matches!(receivers.0.try_recv(), Err(TryRecvError::Empty)),
                "todo"
            );
            assert_eq!(self.commands_overflows, 0);
            self.delayed_commands.push_back(Default::default());
        };

        let delayed = self
            .delayed_commands
            .pop_front()
            .expect("`delayed_commands` should not be empty during `ForwardLogEnd`.");
        if delayed.is_empty() {
            return;
        }

        commands.add(|world: &mut World| {
            world.resource_scope(|world, mut controller: Mut<'_, Self>| {
                let inits = delayed.into_iter().flat_map(|command| command.init(world));
                if let Some(entry) = controller.log.front_mut() {
                    entry.extend(inits);
                } else {
                    panic!("todo");
                }
            });
        })
    }
    fn last_forward_commands_log(&self, mut commands: Commands<'_, '_>) {
        let commands_exist = match self.log.get(self.log_index) {
            Some(v) => !v.is_empty(),
            None => panic!("todo",),
        };
        if commands_exist {
            let log_index = self.log_index;
            commands.add(move |world: &mut World| {
                world.resource_scope(|world, mut controller: Mut<'_, Self>| {
                    let commands = match controller.log.get_mut(log_index) {
                        Some(v) => v,
                        None => panic!("todo"),
                    };
                    commands.iter_mut().for_each(|command| command.redo(world))
                })
            })
        }
    }
    fn apply_progress_query(&mut self, current: ProgressType) {
        /*
                    f f f f b b b b f p f p b p b p
                            !       !       !
        time stamp  0 1 2 3 3 2 1 0 0 0 1 1 1 1 0 0
        log index   3 2 1 0 0 1 2 3
        cmd b/a     a a a a b b b b

        leichter: funktion die am anfang stamp/indices setzt mit after_turn bool

        */
        if self.query.is_none() {
            return;
        }
        let after_forward = match self.current {
            Progress::LogClose { after_forward } => {
                debug_assert_eq!(current, ProgressType::NotLog);
                after_forward
            }
            _ => true,
        };
        match (self.query.unwrap(), current) {
            (ProgressQueried::Forward, ProgressType::NotLog) => {
                self.query = None;
                self.current = Progress::Forward { after_forward }
            }
            (ProgressQueried::Forward, ProgressType::ForwardLog) => {
                self.current = Progress::LogClose {
                    after_forward: true,
                };
            }
            (ProgressQueried::Forward, ProgressType::BackwardLog) => {
                self.current = Progress::LogClose {
                    after_forward: false,
                };
            }
            (
                ProgressQueried::ForwardFast {
                    to_time_stamp,
                    queried,
                },
                ProgressType::NotLog,
            ) => {
                self.query = None;
                if to_time_stamp == self.time_stamp + Wrapping(1) {
                    self.current = Progress::Forward { after_forward };
                } 
                else if to_time_stamp.further_in_the_future(self.time_stamp, queried) {
                    let mut reserve = self.time_stamp.ticks_from_now(to_time_stamp) as usize;
                    if reserve == 0 {
                        reserve = Ticks::MAX as usize + 1;
                    }
                    let mut vd = VecDeque::from_iter((0..reserve).map(|_| Vec::new()));
                    self.delayed_commands.append(&mut vd);
                    self.forward_fast_to = to_time_stamp;
                    self.current = Progress::ForwardFast {
                        after_forward_if_init: Some(after_forward),
                    };
                }
            }
            (ProgressQueried::ForwardFast { .. }, ProgressType::ForwardLog) => {
                self.current = Progress::LogClose {
                    after_forward: true,
                };
            }
            (ProgressQueried::ForwardFast { .. }, ProgressType::BackwardLog) => {
                self.current = Progress::LogClose {
                    after_forward: false,
                };
            }
            (ProgressQueried::ForwardLog, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = Progress::ForwardLog {
                    after_forward: false,
                };
            }
            (ProgressQueried::ForwardLog, _) => {
                self.query = None;
                if !self.log_index_min() {
                    self.current = Progress::ForwardLog {
                        after_forward: true,
                    };
                }
            }
            (ProgressQueried::ForwardLogEnd, ProgressType::NotLog) => {
                self.query = None;
                self.current = match self.log_index {
                    0 => Progress::Pause {
                        after_forward_if_log: None,
                    },
                    1 => Progress::ForwardLog { after_forward },
                    _ => Progress::ForwardLogEnd {
                        after_forward_if_init: Some(after_forward),
                    },
                };
            }
            (ProgressQueried::ForwardLogEnd, ProgressType::ForwardLog) => {
                self.query = None;
                self.current = match self.log_index {
                    0 => Progress::Pause {
                        after_forward_if_log: Some(true),
                    },
                    1 => Progress::ForwardLog {
                        after_forward: true,
                    },
                    _ => Progress::ForwardLogEnd {
                        after_forward_if_init: Some(true),
                    },
                };
            }
            (ProgressQueried::ForwardLogEnd, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = match self.log_index {
                    0 => Progress::ForwardLog {
                        after_forward: false,
                    },
                    _ => Progress::ForwardLogEnd {
                        after_forward_if_init: Some(false),
                    },
                };
            }
            (ProgressQueried::BackwardLog, ProgressType::BackwardLog) => {
                self.query = None;
                if !self.log_index_max() {
                    self.current = Progress::BackwardLog {
                        after_backward: true,
                    };
                }
            }
            (ProgressQueried::BackwardLog, _) => {
                self.query = None;
                self.current = Progress::BackwardLog {
                    after_backward: false,
                };
            }
            (ProgressQueried::BackwardLogEnd, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = match self.log.len() - self.log_index {
                    0 => Progress::Pause {
                        after_forward_if_log: Some(false),
                    },
                    1 => Progress::BackwardLog {
                        after_backward: true,
                    },
                    _ => Progress::BackwardLogEnd {
                        after_backward_if_init: Some(true),
                    },
                };
            }
            (ProgressQueried::BackwardLogEnd, _) => {
                self.query = None;
                self.current = match self.log_index {
                    0 => Progress::BackwardLog {
                        after_backward: false,
                    },
                    _ => Progress::BackwardLogEnd {
                        after_backward_if_init: Some(false),
                    },
                };
            }
            (ProgressQueried::Pause, ProgressType::NotLog) => {
                self.query = None;
                self.current = Progress::Pause {
                    after_forward_if_log: None,
                };
            }
            (ProgressQueried::Pause, ProgressType::ForwardLog) => {
                self.query = None;
                self.current = Progress::Pause {
                    after_forward_if_log: Some(true),
                };
            }
            (ProgressQueried::Pause, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = Progress::Pause {
                    after_forward_if_log: Some(false),
                };
            }
        }
    }
    fn log_index_min(&self) -> bool {
        self.log_index == 0
    }
    fn log_index_max(&self) -> bool {
        self.log_index + 1 == self.log.len()
    }
    fn log_close_split(&mut self, mut commands: Commands<'_, '_>, after_forward: bool) {
        let mut at = self.log_index;
        self.log_index = 0;
        if !after_forward{
            at += 1;
        }
        else if at == 0{
            return;
        }
        let split_off = self.log.split_off(at);
        commands.add(move |world: &mut World| {
            split_off
                .into_iter()
                .flatten()
                .for_each(|command| command.undo_finalize(world))
        });
    }
}
