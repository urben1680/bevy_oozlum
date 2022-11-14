use std::{
    collections::VecDeque,
    iter::FromIterator,
    num::Wrapping,
    ops::RangeInclusive,
    sync::{mpsc::{sync_channel, Receiver, SyncSender, TrySendError}, Mutex},
};

use bevy::{
    ecs::world::Mut,
    log::info,
    prelude::{Commands, ResMut, World, Resource},
    time::Time,
};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks, TicksRelative, ToTimeStamp,
};

use self::{
    consts::ControllerConsts,
    debug::DebugLogContainer,
    progress::{Progress, ProgressLog, ProgressQuery},
};

pub(crate) mod consts;
pub(crate) mod progress;

#[cfg(debug_assertions)]
mod debug;

//#[cfg(test)]
//mod test;

#[cfg(test)]
mod test;

#[derive(Resource)]
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
    pause_at: Option<Wrapping<Ticks>>,

    log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>, //don't use .capacity() instead of the value in constants to keep the data consistent across VecDeque implementation details
    log_index: usize,
    delayed_commands: VecDeque<Vec<Box<dyn ReversibleCommand>>>,
    commands_overflows: u64,
    commands_sender: SyncSender<(usize, Vec<Box<dyn ReversibleCommand>>)>,
    commands_receiver: Mutex<Receiver<(usize, Vec<Box<dyn ReversibleCommand>>)>>,

    #[cfg(debug_assertions)]
    debug: VecDeque<DebugLogContainer>,
    #[cfg(test)]
    constants: ControllerConsts,
}

pub(crate) const fn assert_forward_to_max(forward_to_max: Ticks) {
    if forward_to_max == 0 {
        panic!("`forward_to_max` must not be 0 to take at least the commands of the current step");
    }
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
        after_next: Ticks,
    ) {
        match self.commands_sender.try_send((after_next as usize, v)) {
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
    pub(super) fn new(
        time_stamp: Wrapping<Ticks>,
        log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
        constants: ControllerConsts,
    ) -> Self {
        let (commands_sender, commands_receiver) = sync_channel(constants.sync_sender_capacity);
        Self {
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
            pause_at: None,
            log,
            log_index: 0,
            delayed_commands: VecDeque::with_capacity(constants.delayed_commands_capacity),
            commands_overflows: 0,
            commands_sender,
            commands_receiver: Mutex::new(commands_receiver),

            #[cfg(debug_assertions)]
            debug: VecDeque::with_capacity(constants.debug_capacity),

            #[cfg(test)]
            constants,
        }
    }
    pub(super) fn system_first(mut controller: ResMut<'_, Self>, commands: Commands<'_, '_>) {
        controller.first(commands);
    }
    pub(super) fn system_last(
        mut controller: ResMut<'_, Self>,
        commands: Commands<'_, '_>,
    ) {
        controller.last(commands);
    }
    pub(super) fn first(&mut self, commands: Commands<'_, '_>) {
        match self.progress_current {
            Progress::Forward => {
                if self.time_is_up() {
                    assert_ne!(
                        self.delayed_commands.len(),
                        self.consts().delayed_commands_capacity
                    );
                    self.delayed_commands.push_back(Default::default());
                    self.first_forward_not_log(commands);
                    self.stamps_indices_forward(None);
                    self.first_ran = true;
                    self.update_debug(true);
                }
            }
            Progress::ForwardFast(..) => {
                self.time.update();
                self.first_forward_not_log(commands);
                self.stamps_indices_forward(None);
                self.first_ran = true;
                self.update_debug(true);
            }
            Progress::ForwardLog(after_forward) => {
                if self.time_is_up() {
                    self.stamps_indices_forward(Some(after_forward));
                    self.first_ran = true;
                    self.update_debug(true);
                }
            }
            Progress::ForwardLogFast(after_forward_if_init) => {
                self.stamps_indices_forward(after_forward_if_init.or(Some(true)));
                self.first_ran = true;
                self.update_debug(true);
            }
            Progress::BackwardLog(after_backward) => {
                if self.time_is_up() {
                    self.stamps_indices_backward(after_backward, false);
                    self.first_backward(commands);
                    self.first_ran = true;
                    self.update_debug(true);
                }
            }
            Progress::BackwardLogFast(after_backward_if_init) => {
                self.stamps_indices_backward(after_backward_if_init != Some(false), false);
                self.first_backward(commands);
                self.first_ran = true;
                self.update_debug(true);
            }
            Progress::Pause(..) => {
                self.time.update();
                self.first_ran = true;
                self.update_debug(true);
            }
            Progress::LogClose(after_backward) => {
                self.stamps_indices_backward(after_backward, true);
                self.first_ran = true;
                self.update_debug(true);
            }
        }
    }
    pub(super) fn last(
        &mut self,
        commands: Commands<'_, '_>,
    ) {
        self.first_ran = false;
        self.time_step = self.time_step_query.take().unwrap_or(self.time_step);
        match self.progress_current {
            Progress::Forward => {
                self.progress_current = Progress::Forward;
                self.apply_progress_query(ProgressLog::NotLog);
                if !self.last_forward_commands_sent(commands) {
                    self.update_debug(false);
                }
            }
            Progress::ForwardFast(..) => {
                if self.to_time_stamp.to_time_stamp == self.time_stamp {
                    assert_eq!(self.to_time_stamp.delta_abs, 0);
                    self.progress_current = Progress::Forward;
                    self.apply_progress_query(ProgressLog::NotLog);
                } else {
                    self.progress_current = Progress::ForwardFast(false);
                }
                if !self.last_forward_commands_sent(commands) {
                    self.update_debug(false);
                }
            }
            Progress::ForwardLog(..) => {
                self.last_forward_commands_log(commands);
                self.progress_current = match self.log_index_min() {
                    true => Progress::Pause(Some(true)),
                    false => Progress::ForwardLog(true),
                };
                self.apply_progress_query(ProgressLog::ForwardLog);
                self.update_debug(false);
            }
            Progress::ForwardLogFast(..) => {
                self.last_forward_commands_log(commands);
                if self.to_time_stamp.to_time_stamp == self.time_stamp {
                    assert_eq!(self.to_time_stamp.delta_abs, 0);
                    self.progress_current = Progress::Pause(Some(true));
                    self.apply_progress_query(ProgressLog::ForwardLog);
                } else {
                    self.progress_current = Progress::ForwardLogFast(None);
                }
                self.update_debug(false);
            }
            Progress::BackwardLog(..) => {
                self.progress_current = match self.log_index_max() {
                    true => Progress::Pause(Some(false)),
                    false => Progress::BackwardLog(true),
                };
                self.apply_progress_query(ProgressLog::BackwardLog);
                self.update_debug(false);
            }
            Progress::BackwardLogFast(..) => {
                if self.to_time_stamp.to_time_stamp == self.time_stamp {
                    assert_eq!(self.to_time_stamp.delta_abs, 0);
                    self.progress_current = Progress::Pause(Some(false));
                    self.apply_progress_query(ProgressLog::BackwardLog);
                } else {
                    self.progress_current = Progress::BackwardLogFast(None);
                }
                self.update_debug(false);
            }
            Progress::Pause(after_forward_if_log) => {
                match after_forward_if_log {
                    None => self.apply_progress_query(ProgressLog::NotLog),
                    Some(false) => self.apply_progress_query(ProgressLog::BackwardLog),
                    Some(true) => self.apply_progress_query(ProgressLog::ForwardLog),
                }
                self.update_debug(false);
            }
            Progress::LogClose(..) => {
                self.progress_current = Progress::Forward;
                self.apply_progress_query(ProgressLog::NotLog);
                match self.log_index != 0 {
                    false => self.update_debug(false),
                    true => self.log_close_split(commands),
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
        match after_forward_if_log {
            None => {
                self.time_stamp += 1;
                self.forget += 1;
                self.lower_to_time_stamp_delta();
                assert_eq!(self.log_index, 0);
                assert_ne!(self.log.len(), 0);
            }
            Some(true) => {
                self.time_stamp += 1;
                self.forget += 1;
                self.lower_to_time_stamp_delta();
                self.log_index -= 1;
            }
            _ => (),
        }
    }
    fn stamps_indices_backward(&mut self, after_backward: bool, log_close: bool) {
        if !after_backward {
            return;
        }
        self.time_stamp -= 1;
        self.forget -= 1;
        self.lower_to_time_stamp_delta();
        self.log_index += 1;
        if !log_close {
            //log_index will have a valid index after splitting off
            assert_ne!(self.log_index, self.log.len(), "{:#?}", self.debug);
        }
    }
    fn lower_to_time_stamp_delta(&mut self) {
        self.to_time_stamp.delta_abs = self.to_time_stamp.delta_abs.saturating_sub(1);
    }
    pub fn forward_fast_range(&self) -> RangeInclusive<Wrapping<Ticks>> {
        let start = match self.progress_current {
            Progress::Forward | Progress::ForwardLog(true) => match self.first_ran {
                false => self.time_stamp + Wrapping(2),
                true => self.time_stamp + Wrapping(1),
            },
            Progress::ForwardFast(..)
            | Progress::ForwardLogFast(..)
            | Progress::BackwardLogFast(..) => self.to_time_stamp.to_time_stamp + Wrapping(1),
            Progress::BackwardLog(true) if self.first_ran => self.time_stamp + Wrapping(1),
            _ => self.time_stamp,
        };
        start..=start + Wrapping(self.consts().forward_to_max - 1)
    }
    pub fn log_range(&self) -> RangeInclusive<Wrapping<Ticks>> {
        let mut start = self.log_past_end();
        let mut end = self.log_future_end();
        match self.progress_current {
            Progress::Forward if !self.first_ran => {
                end += 1;
                if start == self.forget - Wrapping(1) {
                    start += 1;
                }
            }
            Progress::ForwardFast(..) => {
                end = self.to_time_stamp.to_time_stamp;
                let start_alt = self.forget + Wrapping(self.to_time_stamp.delta_abs) - Wrapping(1);
                if !start_alt.further_in_the_past(start, end) {
                    start = start_alt;
                }
            }
            Progress::LogClose(..) => {
                //incomplete, inner value, first ran ...
                end = self.time_stamp;
            }
            _ => {}
        }
        start..=end
    }
    pub fn query_progress(
        &mut self,
        query: ProgressQuery,
    ) -> Result<(), RangeInclusive<Wrapping<Ticks>>> {
        self.can_query_progress(query)?;
        self.progress_query = Some(query);
        Ok(())
    }
    pub fn query_progress_command(
        &self,
        commands: &mut Commands<'_, '_>,
        query: ProgressQuery,
    ) -> Result<(), RangeInclusive<Wrapping<Ticks>>> {
        self.can_query_progress(query)?;
        commands.add(move |world: &mut World| {
            world.resource_mut::<Self>().progress_query = Some(query);
        });
        Ok(())
    }
    fn can_query_progress(
        &self,
        query: ProgressQuery,
    ) -> Result<(), RangeInclusive<Wrapping<Ticks>>> {
        match query {
            ProgressQuery::ForwardFastTo(to) => {
                let range = self.forward_fast_range();
                match to.in_range(&range) {
                    false => Err(range),
                    true => Ok(()),
                }
            }
            ProgressQuery::LogTo(to) |
            ProgressQuery::LogFastTo(to) => {
                let range = self.log_range();
                match to.in_range(&range) {
                    false => Err(range),
                    true => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }
    fn query_time_step(&mut self, query: f64) {
        self.time_step_query = Some(query);
    }
    fn first_forward_not_log(&mut self, mut commands: Commands<'_, '_>) {
        if self.log.len() == self.consts().log_capacity {
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
                        entry
                            .iter_mut()
                            .rev()
                            .for_each(|command| command.undo(world));
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
            panic!(
                "todo time_Stamp: {},  first_ran: {:#?}, delayed_commands.len(): {}",
                self.time_stamp,
                self.first_ran,
                self.delayed_commands.len()
            );
        }
    }
    fn last_forward_commands_sent(
        &mut self,
        mut commands: Commands<'_, '_>,
    ) -> bool {
        self.commands_receiver.lock().expect("last access should not have panicked")
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
        let query = match self.progress_query {
            None => return,
            Some(query) => query,
        };
        match query {
            ProgressQuery::ForwardFastTo(to_time_stamp) if progress_log == ProgressLog::NotLog => {
                self.pause_at = None;
                self.progress_query = None;
                if to_time_stamp == self.time_stamp + Wrapping(1) {
                    self.progress_current = Progress::Forward;
                } else {
                    self.update_to_time_stamp(true, Some(to_time_stamp));
                    let reserve = self.to_time_stamp.delta_abs as usize;
                    assert!(
                        reserve + self.delayed_commands.len()
                            <= self.consts().delayed_commands_capacity
                    );
                    let mut vd = VecDeque::from_iter((0..reserve).map(|_| Vec::new()));
                    self.delayed_commands.append(&mut vd);
                    self.progress_current = Progress::ForwardFast(true);
                }
            }
            ProgressQuery::Forward if progress_log == ProgressLog::NotLog => {
                self.pause_at = None;
                self.progress_query = None;
                self.progress_current = Progress::Forward;
            }
            ProgressQuery::Forward | ProgressQuery::ForwardFastTo(..) => {
                self.pause_at = None;
                self.progress_current = Progress::LogClose(progress_log.after_backward());
            }
            ProgressQuery::LogTo(pause_at) => {
                self.progress_query = None;
                let after_forward = progress_log.after_forward();
                self.progress_current = match self.log_to_future_if_not_now(pause_at) {
                    None => Progress::Pause(Some(after_forward)),
                    Some(false) => {
                        self.pause_at = Some(pause_at);
                        Progress::BackwardLog(!after_forward)
                    }
                    Some(true) => {
                        self.pause_at = Some(pause_at);
                        Progress::ForwardLog(after_forward)
                    }
                };
            }
            ProgressQuery::LogFastTo(to_time_stamp) => {
                self.pause_at = None;
                self.progress_query = None;
                let after_forward = progress_log.after_forward();
                self.progress_current = match self.log_to_future_if_not_now(to_time_stamp) {
                    None => Progress::Pause(Some(after_forward)),
                    Some(false) => {
                        self.update_to_time_stamp(false, Some(to_time_stamp));
                        Progress::BackwardLogFast(Some(!after_forward))
                    }
                    Some(true) => {
                        self.update_to_time_stamp(true, Some(to_time_stamp));
                        Progress::ForwardLogFast(Some(after_forward))
                    }
                }
            }
            ProgressQuery::Pause => {
                if self.pause_at == Some(self.time_stamp) {
                    self.pause_at = None;
                }
                self.progress_query = None;
                self.progress_current = Progress::Pause(progress_log.after_forward_if_log());
            }
        }
    }
    fn update_to_time_stamp(&mut self, forward: bool, to: Option<Wrapping<Ticks>>) {
        let to = to.unwrap_or(self.to_time_stamp.to_time_stamp);
        self.to_time_stamp.to_time_stamp = to;
        self.to_time_stamp.delta_abs = match forward {
            false => (self.time_stamp - to).0,
            true => (to - self.time_stamp).0,
        }
    }
    fn log_to_future_if_not_now(&self, to_time_stamp: Wrapping<Ticks>) -> Option<bool> {
        let log_future_end = self.log_future_end();
        let log_past_end = self.log_past_end();
        if !to_time_stamp.in_range(&(log_past_end..=log_future_end)) {
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
                    .rev()
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
