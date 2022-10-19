use std::{
    collections::VecDeque,
    fmt::Debug,
    iter::FromIterator,
    mem::swap,
    num::Wrapping,
    sync::mpsc::{sync_channel, Receiver, SyncSender, TryRecvError, TrySendError},
};

use bevy::{
    ecs::{schedule::ShouldRun, world::Mut},
    log::info,
    prelude::{Commands, Local, NonSendMut, Res, ResMut, World},
    time::Time,
};

use crate::{
    commands::{ReversibleCommand, ReversibleCommandInitialized},
    Ticks, TicksRelative, DEFAULT_TIME_STEP,
};

use self::{
    consts::{ControllerConsts, CONTROLLER_CONSTS},
    progress::Progress,
};

pub(crate) mod consts;
pub(crate) mod progress;

#[cfg(test)]
mod test;

/// `NonSend` resource containing sync channel `Receiver`s for forgets and delayed commands.
pub(super) struct ControllerReceivers {
    /// Messages about commands that are not happening in the next tick, is only sent to if progress is `ForwardFast`.
    commands: Receiver<(usize, Vec<Box<dyn ReversibleCommand>>)>,
}

#[derive(PartialEq)]
pub enum LogEndProximity{
    AtEnd,
    NextTick,
    NotSoon
}

pub struct Controller {
    progress_query: Option<Progress>,
    time_step_query: Option<f64>, //todo: use enum instead with options such as "back to default after n ticks"
    time_step: f64,
    /// Log containing commands to undo or redo them.
    ///
    /// 1. After the stage `Last`, during `Forward` and `ForwardFast`, old commands may be split off and finalized.
    /// 2. After the stage `Last`, during `LogEnd`, new commands may be split off and finalized.
    ///
    /// `VecDeque` has no `split_off_front` method: https://github.com/rust-lang/rust/issues/92547.
    /// The workaround uses needless allocations. So this should be done in the less common case: 2.
    /// Because of this the front of the `VecDeque` is the most recent end and the back the most past end of the log.
    ///
    /// Non-log progresses have to make sure an empty element is inserted before the systems run
    /// because a command writes into it if the SyncChannel is full
    log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
    /// Current time stamp during reversible systems phase
    time_stamp: Wrapping<Ticks>,
    forget: Wrapping<Ticks>,
    forward_fast_limit: Wrapping<Ticks>,
    forward_fast_issued: Wrapping<Ticks>,
    /// Current log index, todo: reverse now that deque ends are reversed!!
    log_index: usize,
    backward: bool,
    progress: Progress,
    //progress_query: Progress,
    pre_update_ran: bool,
    fast_init: bool, //todo make systems
    log_end: bool,
    #[allow(dead_code)]
    commands_sender: SyncSender<(usize, Vec<Box<dyn ReversibleCommand>>)>,
    delayed_commands: VecDeque<Vec<Box<dyn ReversibleCommand>>>,
    commands_overflows: u64,
    //forget_sender: SyncSender<Ticks>,
    //forget_overflow: Option<Ticks>,
    //forget_overflows: u64,
    #[cfg(test)]
    constants: ControllerConsts,
}

impl Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controller")
            .field("progress_query", &self.progress_query)
            .field("time_step_query", &self.time_step_query)
            .field("log.len()", &self.log.len())
            .field("time_stamp", &self.time_stamp)
            .field("forget", &self.forget)
            .field("forward_fast_limit", &self.forward_fast_limit)
            .field("log_index", &self.log_index)
            .field("backward", &self.backward)
            .field("progress", &self.progress)
            .field("pre_update_ran", &self.pre_update_ran)
            .field("fast_init", &self.fast_init)
            .field("log_end", &self.log_end)
            .field("delayed_commands.len()", &self.delayed_commands.len())
            .field("commands_overflows", &self.commands_overflows)
            .finish_non_exhaustive()
    }
}

impl Controller {
    #[cfg(test)]
    pub(super) fn consts(&self) -> &ControllerConsts {
        &self.constants
    }
    #[cfg(not(test))]
    pub(super) const fn consts(&self) -> &ControllerConsts {
        &CONTROLLER_CONSTS
    }
    pub fn forward_fast_limit(&self) -> Wrapping<Ticks> {
        match self.progress {
            Progress::Forward | Progress::ForwardLog => self.time_stamp + Wrapping(1),
            Progress::ForwardFast { to_time_stamp } => to_time_stamp,
            Progress::ForwardLogEnd => {
                self.time_stamp + Wrapping((self.log.len() - self.log_index - 1) as u16)
            } //todo log_index reversed
            Progress::BackwardLog => self.time_stamp - Wrapping(1),
            Progress::BackwardLogEnd => self.time_stamp - Wrapping(self.log_index as Ticks), //todo log_index reversed
            Progress::Pause | Progress::PauseLog => self.time_stamp,
        }
    }
    /// Get the current time stamp.
    pub fn time_stamp(&self) -> Wrapping<Ticks> {
        self.time_stamp
    }
    /// Get the time stamp that is outside the log at the next forward tick.
    pub(super) fn forget(&self) -> Wrapping<Ticks> {
        self.forget
    }
    pub fn log_start(&self) -> Wrapping<Ticks> {
        self.time_stamp - Wrapping(self.log.len() as Ticks)
    }
    /*
    log index   0   1   2
    end b&b     n   s   y
    end b&!b    n   n   s
    end f&b     s   n   n
    end f&!b    y   s   n

    s = soon
    
    */
    pub fn log_front(&self) -> LogEndProximity {
        match (self.backward, self.log_index){
            (false, 0) => LogEndProximity::AtEnd,
            (false, 1) => LogEndProximity::NextTick,
            (true, 0) => LogEndProximity::NextTick,
            _ => LogEndProximity::NotSoon
        }
    }
    pub fn log_back(&self) -> LogEndProximity {
        match (self.backward, self.log.len() - self.log_index){
            (true, 1) => LogEndProximity::AtEnd,
            (true, 2) => LogEndProximity::NextTick,
            (false, 1) => LogEndProximity::NextTick,
            _ => LogEndProximity::NotSoon
        }
    }
    pub fn query_time_step(&mut self, time_step: f64) {
        self.time_step_query = Some(time_step);
    }
    pub fn time_step(&self) -> Option<f64> {
        if self.progress.is_fixed_time_step() {
            Some(self.time_step)
        } else {
            None
        }
    }
    pub fn query_progress_forward_fast_in(&mut self, ticks: Ticks) {
        self.query_progress(Progress::ForwardFast {
            to_time_stamp: self.time_stamp + Wrapping(ticks),
        });
    }
    pub fn query_progress(&mut self, progress: Progress) {
        if matches!(progress, Progress::ForwardFast { .. }) {
            self.forward_fast_issued = self.time_stamp;
        }
        self.progress_query = Some(progress);
    }
    pub fn progress(&self) -> Progress {
        self.progress
    }
    /// Returns numbers of ticks `time_stamp` is in the future.
    pub fn ticks_from_now(&self, time_stamp: Wrapping<Ticks>) -> Ticks {
        time_stamp.ticks_from_now(self.time_stamp)
    }
    /// Returns number of ticks `time_stamp` is in the past.
    pub fn ticks_ago(&self, time_stamp: Wrapping<Ticks>) -> Ticks {
        time_stamp.ticks_ago(self.time_stamp)
    }
    pub fn fast_init(&self) -> bool {
        self.fast_init
    }
    pub(super) fn send_commands(
        &self,
        v: Vec<Box<dyn ReversibleCommand>>,
        commands: &mut Commands<'_, '_>,
    ) {
        assert_eq!(
            self.progress,
            Progress::Forward,
            "`send_commands` should not be called during `progress`: `{:?}`, {self:#?}",
            self.progress
        );
        self.send_commands_raw(v, 0, commands);
    }
    pub(super) fn send_delayed_commands(
        &self,
        v: Vec<Box<dyn ReversibleCommand>>,
        time_stamp: Wrapping<Ticks>,
        commands: &mut Commands<'_, '_>,
    ) {
        assert!(
            matches!(self.progress, Progress::ForwardFast { .. }),
            "`send_commands` should not be called during `progress`: `{:?}`, {self:#?}",
            self.progress
        );
        assert!(
            !time_stamp.further_in_the_future(self.forward_fast_limit, self.time_stamp),
            "`send_commands` should not issue commands for {} which is past `forward_fast_limit`: `{:?}`, {self:#?}",
            time_stamp, self.forward_fast_limit
        );
        let index = self.ticks_from_now(time_stamp) as usize;
        self.send_commands_raw(v, index, commands);
    }
    fn send_commands_raw(
        &self,
        v: Vec<Box<dyn ReversibleCommand>>,
        index: usize,
        commands: &mut Commands<'_, '_>,
    ) {
        match self.commands_sender.try_send((index, v)) {
            Ok(_) => {}
            Err(TrySendError::Full((index, command))) => commands.add(move |world: &mut World| {
                let mut controller = world.resource_mut::<Self>();
                controller.commands_overflows += 1;
                controller.add_delayed_commands(index, command);
            }),
            Err(TrySendError::Disconnected(_)) => {
                panic!("Could not send delayed command, receiver disconnected.")
            }
        }
    }

    /// Add new command.
    pub(super) fn push_command<I: Iterator<Item = Box<dyn ReversibleCommandInitialized>>>(
        &mut self,
        commands: I,
    ) {
        debug_assert!(
            matches!(
                self.progress,
                Progress::Forward | Progress::ForwardFast { .. }
            ),
            "{self:#?}"
        );
        self.log
            .front_mut()
            .expect("`log` should not be empty")
            .extend(commands);
    }
    fn forward_stamps(&mut self) {
        self.time_stamp += 1;
        self.forget += 1;
    }
    fn backward_stamps(&mut self) {
        self.time_stamp -= 1;
        self.forget -= 1;
    }
    fn forward_log_index(&mut self) {
        if self.backward{
            self.backward = false;
        }
        else {
            self.log_index -= 1;
        }
    }
    fn backward_log_index_successful(&mut self) -> bool {
        if !self.backward{
            self.backward = true;
            self.log_index += 1;
        }
        else if self.log_index + 1 == self.log.len(){
            return false;
        }
        else {
            self.log_index += 1;
        }
        true
    }
    fn add_delayed_commands(
        &mut self,
        index: usize,
        mut commands: Vec<Box<dyn ReversibleCommand>>,
    ) {
        match self.delayed_commands.get_mut(index) {
            Some(v) => v.append(&mut commands),
            None => {
                panic!(
                "`delayed_commands` with `len` {} is too short for `index` {}, `len` should be {}. {self:#?}",
                self.log.len(), index, self.ticks_from_now(self.forward_fast_limit())
            )
            }
        }
    }
    fn apply_progress_query(&mut self, or: Progress) {
        let mut query = self.progress_query.take().unwrap_or(or);
        match query {
            Progress::ForwardFast { to_time_stamp } => {
                if to_time_stamp == self.time_stamp() + Wrapping(1) {
                    query = Progress::Forward;
                } else if !to_time_stamp
                    .further_in_the_future(self.time_stamp, self.forward_fast_issued)
                {
                    query = or;
                } else {
                    let mut reserve = self.ticks_from_now(to_time_stamp) as usize;
                    if reserve == 0 {
                        reserve = Ticks::MAX as usize + 1;
                    }
                    self.fast_init = true;
                    let mut vd = VecDeque::from_iter((0..reserve).map(|_| Vec::new()));
                    self.delayed_commands.append(&mut vd);
                    self.forward_fast_limit = to_time_stamp;
                }
            }
            Progress::ForwardLogEnd => {
                match self.log_front(){
                    LogEndProximity::AtEnd => query = Progress::PauseLog,
                    LogEndProximity::NextTick => query = Progress::ForwardLog,
                    LogEndProximity::NotSoon => self.fast_init = true
                }
            }
            Progress::BackwardLogEnd => {
                match self.log_back(){
                    LogEndProximity::AtEnd => query = Progress::PauseLog,
                    LogEndProximity::NextTick => query = Progress::BackwardLog,
                    LogEndProximity::NotSoon => self.fast_init = true
                }
            }
            Progress::ForwardLog => {
                if self.log_front() == LogEndProximity::AtEnd{
                    query = Progress::PauseLog;
                }
            }
            Progress::BackwardLog => {
                if self.log_back() == LogEndProximity::AtEnd{
                    query = Progress::PauseLog;
                }
            }
            _ => {}
        }
        self.log_end = self.progress.is_log(true) && !query.is_log(true);
        self.progress = query;
    }
}

/// Run criterias for other systems
impl Controller {
    pub fn run_criteria<
        const LOG: bool,
        const PAUSE: bool,
        const LOG_PAUSE: bool,
        const LOG_END: bool,
    >(
        controller: Res<'_, Self>,
    ) -> ShouldRun {
        (match (LOG, PAUSE, LOG_PAUSE) {
            (false, false, false) => controller.progress.is_not_log_nor_pause(),
            (false, false, true) => controller.progress == Progress::PauseLog,
            (false, true, false) => controller.progress == Progress::Pause,
            (false, true, true) => controller.progress.is_pause(),
            (true, false, false) => controller.progress.is_log(false),
            (true, false, true) => controller.progress.is_log(true),
            (true, true, false) => {
                controller.progress.is_log(false) || controller.progress == Progress::Pause
            }
            (true, true, true) => !controller.progress.is_not_log_nor_pause(),
        } || (LOG_END && controller.log_end))
            .into()
    }
    pub fn run_criteria_forward(controller: Res<'_, Self>) -> ShouldRun {
        controller.after_pre_update_not_log_end(controller.progress == Progress::Forward)
    }
    pub fn run_criteria_forward_fast(controller: Res<'_, Self>) -> ShouldRun {
        controller.after_pre_update_not_log_end(matches!(
            controller.progress,
            Progress::ForwardFast { .. }
        ))
    }
    pub fn run_criteria_forward_log(controller: Res<'_, Self>) -> ShouldRun {
        controller.after_pre_update_not_log_end(controller.progress == Progress::ForwardLog)
    }
    pub fn run_criteria_forward_log_fast(controller: Res<'_, Self>) -> ShouldRun {
        controller.after_pre_update_not_log_end(controller.progress == Progress::ForwardLogEnd)
    }
    pub fn run_criteria_backward_log(controller: Res<'_, Self>) -> ShouldRun {
        controller.after_pre_update_not_log_end(controller.progress == Progress::BackwardLog)
    }
    pub fn run_criteria_backward_log_fast(controller: Res<'_, Self>) -> ShouldRun {
        controller.after_pre_update_not_log_end(controller.progress == Progress::BackwardLogEnd)
    }
    pub fn run_criteria_log_end(controller: Res<'_, Self>) -> ShouldRun {
        controller.after_pre_update(controller.log_end)
    }
    fn after_pre_update_not_log_end(&self, b: bool) -> ShouldRun {
        self.after_pre_update(b && !self.log_end)
    }
    fn after_pre_update(&self, b: bool) -> ShouldRun {
        (b && self.pre_update_ran).into()
    }
}

/// Systems and commands
impl Controller {
    /// Command to load `Controller` and `ControllerReceivers`.
    pub(super) fn insert_command(
        log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
        time_stamp: Wrapping<Ticks>,
        world: &mut World,
        constants: ControllerConsts,
    ) {
        #[cfg(not(test))]
        assert_eq!(constants, CONTROLLER_CONSTS);
        let (commands_s, commands_r) =
            sync_channel(constants.delayed_commands_sync_sender_capacity);
        world.insert_non_send_resource(ControllerReceivers {
            commands: commands_r,
        });
        let mut delayed_commands = VecDeque::with_capacity(constants.delayed_events_ticks_capacity);
        delayed_commands.push_front(Default::default());
        world.insert_resource(Self {
            time_step_query: None,
            progress_query: None,
            time_step: DEFAULT_TIME_STEP,
            log,
            time_stamp,
            forget: time_stamp - Wrapping(constants.max_log_index),
            forward_fast_limit: Default::default(), //only needs a valid value during fast forward progress
            forward_fast_issued: Default::default(), //same
            log_index: 0,
            backward: false,
            progress: Progress::Forward,
            pre_update_ran: false,
            log_end: false,   //log_end systems must have been run before save
            fast_init: false, //cannot occure just before `Forward`
            commands_sender: commands_s,
            delayed_commands,
            commands_overflows: 0,
            #[cfg(test)]
            constants,
        })
    }
    /// System that should be run before all reversible systems.
    pub(super) fn first_system(
        mut controller: ResMut<'_, Self>,
        time: Local<'_, Time>,
        elapsed: Local<'_, f64>,
        commands: Commands<'_, '_>,
    ) {
        if !controller.first_early_return(time, elapsed) {
            controller.first_update(commands);
        }
    }
    /// System that should be run after all reversible systems.
    pub(super) fn last_system(
        mut controller: ResMut<'_, Self>,
        receivers: NonSendMut<'_, ControllerReceivers>,
        commands: Commands<'_, '_>,
    ) {
        if controller.pre_update_ran {
            controller.last(receivers, commands);
        } else {
        }
    }
    fn first_early_return(
        &mut self,
        mut time: Local<'_, Time>,
        mut elapsed: Local<'_, f64>,
    ) -> bool {
        if self.log_end {
            self.pre_update_ran = true;
            return true;
        }
        match self.progress {
            Progress::Pause | Progress::PauseLog => {
                time.update();
                return true;
            }
            Progress::Forward | Progress::ForwardLog | Progress::BackwardLog => {
                *elapsed -= time.delta_seconds_f64();
                time.update();
                if *elapsed > 0.0 {
                    return true;
                }
                *elapsed += self.time_step;
                self.pre_update_ran = true;
            }
            Progress::ForwardFast { .. } | Progress::ForwardLogEnd | Progress::BackwardLogEnd => {}
        }
        self.pre_update_ran = true;
        false
    }
    fn first_update(&mut self, mut commands: Commands<'_, '_>) {
        match self.progress {
            Progress::Forward | Progress::ForwardFast { .. } => {
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
            Progress::ForwardLog | Progress::ForwardLogEnd => {
                self.forward_log_index();
            }
            Progress::BackwardLog | Progress::BackwardLogEnd => {
                self.backward_stamps();
                match self.log.get(self.log_index) {
                    Some(v) => {
                        if !v.is_empty() {
                            commands.add(|world: &mut World|{
                                world.resource_scope(|world, mut controller: Mut<'_, Self>|{
                                    let log_index = controller.log_index;
                                    match controller.log.get_mut(log_index){
                                        Some(entry) => {
                                            entry.iter_mut().for_each(|command|command.undo(world));
                                        },
                                        None => {
                                            panic!("`log` with `len` {} is too short for `log_index` {} in `first_update` method command. {controller:#?}", controller.log.len(), controller.log_index);
                                        }
                                    }
                                });
                            });
                        }
                    }
                    None => {
                        panic!("`log` with `len` {} is too short for `log_index` {} in `first_update` method. {self:#?}", self.log.len(), self.log_index);
                    }
                }
            }
            Progress::Pause | Progress::PauseLog => {}
        }
    }
    fn last(&mut self, receivers: NonSendMut<'_, ControllerReceivers>, commands: Commands<'_, '_>) {
        self.pre_update_ran = false;
        if let Some(time_step) = self.time_step_query.take() {
            self.time_step = time_step;
        }
        if self.log_end {
            self.log_end(commands);
            return;
        }
        self.fast_init = false; //is set to true later in `apply_progress_query` if appropiate
        match self.progress {
            Progress::Forward => {
                self.forward_commands(receivers, commands, false);
                self.forward_stamps();
                self.apply_progress_query(self.progress);
            }
            Progress::ForwardFast { to_time_stamp } => {
                self.forward_commands(receivers, commands, true);
                self.forward_stamps();
                if to_time_stamp == self.time_stamp {
                    self.apply_progress_query(Progress::Forward);
                }
            }
            Progress::ForwardLog => {
                self.forward_log_commands(commands);
                self.forward_stamps();
                self.apply_progress_query(Progress::ForwardLog);
            }
            Progress::ForwardLogEnd => {
                self.forward_log_commands(commands);
                self.forward_stamps();
                if self.log_front() == LogEndProximity::AtEnd {
                    self.apply_progress_query(Progress::PauseLog);
                }
            }
            Progress::BackwardLog => {
                if self.backward_log_index_successful(){
                    self.apply_progress_query(Progress::BackwardLog);
                }
                else {
                    self.apply_progress_query(Progress::PauseLog);
                }
            }
            Progress::BackwardLogEnd => {
                if !self.backward_log_index_successful(){
                    self.apply_progress_query(Progress::PauseLog);
                }
            }
            Progress::PauseLog | Progress::Pause => {
                self.apply_progress_query(self.progress);
            }
        }
    }
    fn forward_commands(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceivers>,
        mut commands: Commands<'_, '_>,
        fast_forward: bool,
    ) {
        if fast_forward {
            receivers
                .commands
                .try_iter()
                .for_each(|(index, command)| self.add_delayed_commands(index, command));
            if self.commands_overflows != 0 {
                info!(
                    "`delayed_commands_overflows` {} with DELAYED_COMMANDS_SYNC_SENDER_CAPACITY {}",
                    self.commands_overflows,
                    self.consts().delayed_commands_sync_sender_capacity
                );
                self.commands_overflows = 0;
            }
            /*
            if self.forward_fast_limit == self.time_stamp + Wrapping(1){
                assert_eq!(self.delayed_commands.len(), 1, "{self:#?}");
                self.delayed_commands.push_back(Default::default());
            } else {
                assert!(self.delayed_commands.len() > 1, "{self:#?}");
            }
            */
        } else {
            debug_assert!(
                matches!(receivers.commands.try_recv(), Err(TryRecvError::Empty)),
                "{self:#?}"
            );
            if !self.fast_init {
                assert_eq!(self.delayed_commands.len(), 1, "{self:#?}");
            }
            debug_assert_eq!(self.commands_overflows, 0);
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
                    panic!("`log` should not be empty to insert commands. {controller:#?}");
                }
            });
        })
    }
    fn forward_log_commands(&self, mut commands: Commands<'_, '_>) {
        let commands_exist = match self.log.get(self.log_index) {
            Some(v) => !v.is_empty(),
            None => panic!(
                "`log` with `len` {} is too short for `log_index` {} during `ForwardLog` system. {self:#?}",
                self.log.len(),
                self.log_index
            ),
        };
        if commands_exist {
            let log_len = self.log.len();
            let log_index = self.log_index;
            commands.add(move |world: &mut World|{
                world.resource_scope(|world, mut controller: Mut<'_, Self>|{
                    let commands = match controller.log.get_mut(log_index){
                        Some(v) => v,
                        None => panic!("`log` with `len` {} is too short for `log_index` {} during `ForwardLog` command. {controller:#?}", log_len, log_index)
                    };
                    commands.iter_mut().for_each(|command| command.redo(world))
                })
            })
        }
    }
    fn log_end(&mut self, mut commands: Commands<'_, '_>) {
        self.log_end = false;
        if matches!(self.progress_query, Some(progress) if progress.is_log(true)) {
            self.progress_query = None;
        }
        if self.log_front() == LogEndProximity::AtEnd {
            return;
        }
        //workaround for not having `VecDeque::split_off_front`, see https://github.com/rust-lang/rust/issues/92547
        let additional = self.log.len() - self.log_index;
        let mut split_off = self.log.split_off(self.log_index);
        split_off.reserve_exact(additional);
        swap(&mut split_off, &mut self.log);
        //end of workaround
        self.log_index = 0;
        self.backward = false;
        commands.add(move |world: &mut World| {
            split_off
                .into_iter()
                .flatten()
                .for_each(|command| command.undo_finalize(world))
        });
    }
}
