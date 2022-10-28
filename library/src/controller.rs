use std::{
    collections::VecDeque,
    iter::FromIterator,
    num::Wrapping,
    sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError}, ops::RangeInclusive,
};

use bevy::{
    ecs::world::Mut,
    log::info,
    prelude::{Commands, NonSendMut, ResMut, World, Res},
    time::Time,
};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks, TicksRelative, ToTimeStamp, log_systems::{per_system::PerSystem, NextTransition},
};

use self::{
    consts::ControllerConsts,
    debug::DebugLogContainer,
    progress::{Progress, ProgressLog, ProgressQuery, ProgressQueryError, QueryLimit},
};

pub(crate) mod consts;
pub(crate) mod progress;

#[cfg(debug_assertions)]
mod debug;

#[cfg(test)]
mod test;

/// `NonSend` resource containing sync channel `Receiver`s for forgets and delayed commands.
pub(super) struct ControllerReceiver(Receiver<(usize, Vec<Box<dyn ReversibleCommand>>)>);

pub(super) struct Controller {
    time_step_query: Option<f64>, //todo: use enum instead with options such as "back to default after n ticks"
    time_step: f64,
    elapsed: f64,
    time: Time,
    first_ran: bool,

    progress_current: Progress,
    progress_query: Option<ProgressQuery>,
    time_stamp: Wrapping<Ticks>,
    forget: Wrapping<Ticks>,
    to_time_stamp: ToTimeStamp,

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
    pub(super) fn to_time_stamp(&self) -> ToTimeStamp {
        self.to_time_stamp
    }
    pub(super) fn log_past_end(&self) -> Wrapping<Ticks> {
        self.time_stamp - Wrapping((self.log.len() - self.log_index) as Ticks)
    }
    pub(super) fn log_future_end(&self) -> Wrapping<Ticks> {
        self.time_stamp + Wrapping(self.log_index as Ticks)
    }
    pub(super) fn send_commands(
        &self,
        v: Vec<Box<dyn ReversibleCommand>>,
        commands: &mut Commands<'_, '_>,
        from_now: Ticks,
    ) {
        assert!(from_now <= self.consts().fast_forward_max, "todo");
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
            progress_current: Progress::Forward,
            progress_query: None,
            time_stamp,
            forget: time_stamp - Wrapping(constants.max_log_index),
            to_time_stamp: Default::default(),
            log,
            log_index: 0,
            delayed_commands: VecDeque::with_capacity(constants.fast_forward_max as usize),
            commands_overflows: 0,
            commands_sender,

            #[cfg(debug_assertions)]
            debug: VecDeque::with_capacity(constants.debug_capacity),

            #[cfg(test)]
            constants,
        });
    }
    pub(super) fn system_first(mut controller: ResMut<'_, Self>, commands: Commands<'_, '_>) {
        controller.first(commands);
    }
    pub(super) fn system_last(
        mut controller: ResMut<'_, Self>,
        receivers: NonSendMut<'_, ControllerReceiver>,
        commands: Commands<'_, '_>,
    ) {
        controller.last(receivers, commands);
    }
    pub(super) fn first(&mut self, commands: Commands<'_, '_>) {
        match self.progress_current {
            Progress::Forward => {
                if self.time_is_up() {
                    self.delayed_commands.push_back(Default::default());
                    self.first_forward_not_log(commands);
                    self.stamps_indices_forward(None);
                    self.first_ran = true;
                }
            }
            Progress::ForwardTo { .. } => {
                self.time.update();
                self.first_forward_not_log(commands);
                self.stamps_indices_forward(None);
                self.first_ran = true;
            }
            Progress::ForwardLog { after_forward } => {
                if self.time_is_up() {
                    self.stamps_indices_forward(Some(after_forward));
                    self.first_ran = true;
                }
            }
            Progress::ForwardLogTo {
                after_forward_if_init,
            } => {
                self.stamps_indices_forward(after_forward_if_init.or(Some(true)));
                self.first_ran = true;
            }
            Progress::BackwardLog { after_backward } => {
                if self.time_is_up() {
                    self.stamps_indices_backward(after_backward);
                    self.first_backward(commands);
                    self.first_ran = true;
                }
            }
            Progress::BackwardLogTo {
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
            Progress::LogClose { after_backward } => {
                self.stamps_indices_backward(after_backward);
                self.first_ran = true;
            }
        }
        #[cfg(debug_assertions)]
        self.update_debug(true);
    }
    pub(super) fn last(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceiver>,
        commands: Commands<'_, '_>,
    ) {
        self.first_ran = false;
        self.time_step = self.time_step_query.take().unwrap_or(self.time_step);
        match self.progress_current {
            Progress::Forward { .. } => {
                self.progress_current = Progress::Forward;
                self.apply_progress_query(ProgressLog::NotLog);
                if !self.last_forward_commands_sent(receivers, commands){
                    self.update_debug(false);
                }
            }
            Progress::ForwardTo { .. } => {
                if self.to_time_stamp.to_time_stamp == self.time_stamp {
                    self.progress_current = Progress::Forward;
                    self.apply_progress_query(ProgressLog::NotLog);
                } else {
                    self.progress_current = Progress::ForwardTo {
                        init: false
                    };
                }
                if !self.last_forward_commands_sent(receivers, commands){
                    self.update_debug(false);
                }
            }
            Progress::ForwardLog { .. } => {
                self.last_forward_commands_log(commands);
                self.progress_current = match self.log_index_min() {
                    true => Progress::Pause {
                        after_forward_if_log: Some(true),
                    },
                    false => Progress::ForwardLog {
                        after_forward: true,
                    },
                };
                self.apply_progress_query(ProgressLog::ForwardLog);
                self.update_debug(false);
            }
            Progress::ForwardLogTo { .. } => {
                self.last_forward_commands_log(commands);
                if self.to_time_stamp.to_time_stamp == self.time_stamp {
                    self.progress_current = Progress::Pause {
                        after_forward_if_log: Some(true),
                    };
                    self.apply_progress_query(ProgressLog::ForwardLog);
                } else {
                    self.progress_current = Progress::ForwardLogTo {
                        after_forward_if_init: None,
                    };
                }
                self.update_debug(false);
            }
            Progress::BackwardLog { .. } => {
                self.progress_current = match self.log_index_max() {
                    true => Progress::Pause {
                        after_forward_if_log: Some(false),
                    },
                    false => Progress::BackwardLog {
                        after_backward: true,
                    },
                };
                self.apply_progress_query(ProgressLog::BackwardLog);
                self.update_debug(false);
            }
            Progress::BackwardLogTo { .. } => {
                if self.to_time_stamp.to_time_stamp == self.time_stamp {
                    self.progress_current = Progress::Pause {
                        after_forward_if_log: Some(false),
                    };
                    self.apply_progress_query(ProgressLog::BackwardLog);
                } else {
                    self.progress_current = Progress::BackwardLogTo {
                        after_backward_if_init: None,
                    };
                }
                self.update_debug(false);
            }
            Progress::Pause {
                after_forward_if_log,
            } => {
                match after_forward_if_log {
                    None => self.apply_progress_query(ProgressLog::NotLog),
                    Some(false) => self.apply_progress_query(ProgressLog::BackwardLog),
                    Some(true) => self.apply_progress_query(ProgressLog::ForwardLog),
                }
                self.update_debug(false);
            },
            Progress::LogClose { .. } => {
                self.progress_current = Progress::Forward;
                self.apply_progress_query(ProgressLog::NotLog);
                match self.log_index != 0{
                    false => self.update_debug(false),
                    true => self.log_close_split(commands)
                }
            }
        }
    }
    pub(super) fn update_debug(&mut self, after_first: bool) {
        #[cfg(debug_assertions)]
        {
            let after = (&*self).into();
            if after_first {
                if self.debug.len() == self.debug.capacity() {
                    self.debug.pop_back();
                }
                self.debug.push_front(DebugLogContainer {
                    after_first: after,
                    after_last: None,
                });
            } else {
                let front = self.debug.front_mut().unwrap();
                front.after_last = Some(after);
            }
        }
    }
    fn stamps_indices_forward(&mut self, after_forward_if_log: Option<bool>) {
        match after_forward_if_log{
            None => {
                self.time_stamp += 1;
                self.forget += 1;
            }
            Some(false) => {},
            Some(true) => {
                self.time_stamp += 1;
                self.forget += 1;
                self.log_index -= 1;
            }
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
    pub fn query_progress(&mut self, query: ProgressQuery) -> Result<(), ProgressQueryError> {
        self.can_query_progress(query)?;
        self.progress_query = Some(query);
        Ok(())
    }
    pub fn query_procress_command(&self, mut commands: Commands<'_, '_>, query: ProgressQuery) -> Result<(), ProgressQueryError>{
        self.can_query_progress(query)?;
        commands.add(move |world: &mut World|{
            world.resource_mut::<Self>().progress_query = Some(query);
        });
        Ok(())
    }
    pub fn query_limit(&self) -> QueryLimit{
        match self.progress_current{
            Progress::ForwardTo { .. } 
            | Progress::ForwardLogTo { .. } 
            | Progress::BackwardLogTo { .. } => QueryLimit::CurrentlyNotQueryable,
            Progress::Forward if !self.first_ran => {
                let mut past = self.log_past_end();
                if self.log.len() == self.consts().log_len{
                    past += 1;
                }
                QueryLimit::CurrentLimit { 
                    forward_to_panic: self.time_stamp + Wrapping(1), 
                    log_to_range: past..=(self.log_future_end() + Wrapping(1))
                }
            },
            Progress::ForwardLog { after_forward: true } if !self.first_ran => {
                QueryLimit::CurrentLimit { 
                    forward_to_panic: self.time_stamp + Wrapping(1), 
                    log_to_range: self.log_past_end()..=self.log_future_end()
                }
            },
            Progress::BackwardLog { after_backward: true } 
            | Progress::LogClose { after_backward: true } if !self.first_ran => {
                QueryLimit::CurrentLimit { 
                    forward_to_panic: self.time_stamp - Wrapping(1), 
                    log_to_range: self.log_past_end()..=self.log_future_end()
                }
            },
            _ => QueryLimit::CurrentLimit { 
                forward_to_panic: self.time_stamp, 
                log_to_range: self.log_past_end()..=self.log_future_end()
            }
        }
    }
    fn can_query_progress(&self, query: ProgressQuery) -> Result<(), ProgressQueryError>{
        if let QueryLimit::CurrentLimit { forward_to_panic, log_to_range } = self.query_limit(){
            match query{
                ProgressQuery::ForwardTo(to) if forward_to_panic == to => Err(ProgressQueryError::QueryFortwardToPresent),
                ProgressQuery::LogTo(to) if !to.in_range(&log_to_range) => Err(ProgressQueryError::QueryOutOfRange(*log_to_range.start()..=*log_to_range.end())),
                _ => Ok(())
            }
        }
        else{
            Err(ProgressQueryError::ForwardToOrLogTo)
        }
    }
    fn query_time_step(&mut self, query: f64) {
        self.time_step_query = Some(query);
    }
    fn first_forward_not_log(&mut self, mut commands: Commands<'_, '_>) {
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
    fn first_backward(&mut self, mut commands: Commands<'_, '_>) {
        if let Some(v) = self.log.get(self.log_index) {
            if v.is_empty() {
                return;
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
    fn time_is_up(&mut self) -> bool {
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
            panic!("todo time_Stamp: {},  first_ran: {:#?}", self.time_stamp, self.first_ran);
        }
    }
    fn last_forward_commands_sent(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceiver>,
        mut commands: Commands<'_, '_>,
    ) -> bool {
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

        let delayed = self
            .delayed_commands
            .pop_front()
            .expect("`delayed_commands` should not be empty");

        if delayed.is_empty() {
            return false;
        }

        commands.add(|world: &mut World| {
            world.resource_scope(|world, mut controller: Mut<'_, Self>| {
                let inits = delayed.into_iter().flat_map(|command| command.init(world));
                if let Some(entry) = controller.log.front_mut() {
                    entry.extend(inits);
                } else {
                    panic!("todo");
                }
                controller.update_debug(false);
            });
        });

        true
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
    fn apply_progress_query(&mut self, progress_log: ProgressLog) {
        #[cfg(test)]
        {
            self.to_time_stamp = Default::default();
        }
        if self.progress_query.is_none() {
            return;
        }
        match (self.progress_query.unwrap(), progress_log) {
            (ProgressQuery::Forward, ProgressLog::NotLog) => {
                self.progress_query = None;
                self.progress_current = Progress::Forward;
            }
            (ProgressQuery::Forward, ProgressLog::ForwardLog)
            | (ProgressQuery::ForwardTo ( .. ), ProgressLog::ForwardLog) => {
                self.progress_current = Progress::LogClose {
                    after_backward: false,
                };
            }
            (ProgressQuery::Forward, ProgressLog::BackwardLog)
            | (ProgressQuery::ForwardTo ( .. ), ProgressLog::BackwardLog) => {
                self.progress_current = Progress::LogClose {
                    after_backward: true,
                };
            }
            (
                ProgressQuery::ForwardTo ( to_time_stamp ),
                ProgressLog::NotLog,
            ) => {
                self.progress_query = None;
                if to_time_stamp == self.time_stamp + Wrapping(1) {
                    self.progress_current = Progress::Forward;
                } else {
                    let reserve = to_time_stamp.ticks_from_now(self.time_stamp) as usize;
                    let mut vd = VecDeque::from_iter((0..reserve).map(|_| Vec::new()));
                    self.delayed_commands.append(&mut vd);
                    self.to_time_stamp = ToTimeStamp::to_future(self.time_stamp, to_time_stamp);
                    self.progress_current = Progress::ForwardTo {
                        init: true
                    };
                }
            }
            (ProgressQuery::ForwardLog, ProgressLog::NotLog) => {
                assert!(self.log_index_min());
                self.progress_query = None;
                self.progress_current = Progress::Pause {
                    after_forward_if_log: Some(true),
                };
            }
            (ProgressQuery::ForwardLog, ProgressLog::ForwardLog) => {
                self.progress_query = None;
                if !self.log_index_min() {
                    self.progress_current = Progress::ForwardLog {
                        after_forward: true,
                    };
                }
            }
            (ProgressQuery::ForwardLog, ProgressLog::BackwardLog) => {
                self.progress_query = None;
                self.progress_current = Progress::ForwardLog {
                    after_forward: false,
                };
            }
            (ProgressQuery::BackwardLog, ProgressLog::BackwardLog) => {
                self.progress_query = None;
                if !self.log_index_max() {
                    self.progress_current = Progress::BackwardLog {
                        after_backward: true,
                    };
                }
            }
            (ProgressQuery::BackwardLog, _) => {
                self.progress_query = None;
                self.progress_current = Progress::BackwardLog {
                    after_backward: false,
                };
            }
            (ProgressQuery::LogTo(to_time_stamp), ProgressLog::NotLog) => {
                self.progress_query = None;
                self.progress_current = if self.log_to_future_if_not_now(to_time_stamp).is_none() {
                    Progress::Pause {
                        after_forward_if_log: Some(true),
                    }
                } else {
                    self.to_time_stamp = ToTimeStamp::to_past(self.time_stamp, to_time_stamp);
                    Progress::BackwardLogTo {
                        after_backward_if_init: Some(false),
                    }
                };
            }
            (ProgressQuery::LogTo(to_time_stamp), ProgressLog::ForwardLog) => {
                self.progress_query = None;
                self.progress_current = match self.log_to_future_if_not_now(to_time_stamp) {
                    None => Progress::Pause {
                        after_forward_if_log: Some(true),
                    },
                    Some(false) => {
                        self.to_time_stamp = ToTimeStamp::to_past(self.time_stamp, to_time_stamp);
                        Progress::BackwardLogTo {
                            after_backward_if_init: Some(false),
                        }
                    }
                    Some(true) => {
                        self.to_time_stamp = ToTimeStamp::to_future(self.time_stamp, to_time_stamp);
                        Progress::ForwardLogTo {
                            after_forward_if_init: Some(true),
                        }
                    }
                };
            }
            (ProgressQuery::LogTo(to_time_stamp), ProgressLog::BackwardLog) => {
                self.progress_query = None;
                self.progress_current = match self.log_to_future_if_not_now(to_time_stamp) {
                    None => Progress::Pause {
                        after_forward_if_log: Some(false),
                    },
                    Some(false) => {
                        self.to_time_stamp = ToTimeStamp::to_past(self.time_stamp, to_time_stamp);
                        Progress::BackwardLogTo {
                            after_backward_if_init: Some(true),
                        }
                    }
                    Some(true) => {
                        self.to_time_stamp = ToTimeStamp::to_future(self.time_stamp, to_time_stamp);
                        Progress::ForwardLogTo {
                            after_forward_if_init: Some(false),
                        }
                    }
                };
            }
            (ProgressQuery::Pause, ProgressLog::NotLog) => {
                self.progress_query = None;
                self.progress_current = Progress::Pause {
                    after_forward_if_log: None,
                };
            }
            (ProgressQuery::Pause, ProgressLog::ForwardLog) => {
                self.progress_query = None;
                self.progress_current = Progress::Pause {
                    after_forward_if_log: Some(true),
                };
            }
            (ProgressQuery::Pause, ProgressLog::BackwardLog) => {
                self.progress_query = None;
                self.progress_current = Progress::Pause {
                    after_forward_if_log: Some(false),
                };
            }
        }
    }
    fn log_to_future_if_not_now(&self, to_time_stamp: Wrapping<Ticks>) -> Option<bool> {
        let log_future_end = self.log_future_end();
        let log_past_end = self.log_past_end();
        if to_time_stamp.further_in_the_future(log_future_end, log_past_end)
            || to_time_stamp.further_in_the_past(log_past_end, log_future_end)
        {
            panic!("`ProgressQueried::LogTo(Wrapping({to_time_stamp}))` out of range of `Wrapping({log_past_end})..=Wrapping({log_future_end})`.");
        }
        if to_time_stamp == self.time_stamp {
            None
        } else if to_time_stamp.further_in_the_future(self.time_stamp, log_past_end) {
            Some(true)
        } else {
            Some(false)
        }
    }
    fn log_index_min(&self) -> bool {
        self.log_index == 0
    }
    fn log_index_max(&self) -> bool {
        self.log_index + 1 == self.log.len()
    }
    fn log_close_split(&mut self, mut commands: Commands<'_, '_>) {
        commands.add(move |world: &mut World| {
            world.resource_scope(move |world, mut controller: Mut<'_, Self>| {
                (0..controller.log_index)
                    .flat_map(|_| controller.log.pop_front().expect("todo"))
                    .for_each(|commands| commands.undo_finalize(world));
                controller.log_index = 0;
                controller.update_debug(false);
            });
        });
    }
}

/*
Commands logik:

- Während den Systemen, also bei controller.first_ran == true, werden reversible commands gesendet:
-- bei Forward in delayed_commands[0], also muss in diesem progress immer ein element vorhanden sein
-- bei ForwardTo in delayed_commands[x], also muss in diesem progress immer genug elemente für den ganzen Sprung vorhanden sein
-- bei allen anderen ist delayed_commands unwichtig
- am ende der Systeme, in der controller.last(), wird delayed_commands[0] entnommen und initialisiert und ggf in den log eingefügt

*/
