use std::{num::Wrapping, collections::VecDeque, sync::mpsc::{SyncSender, TryRecvError, Receiver, TrySendError}};

use bevy::{prelude::{Commands, World, Mut, NonSendMut, info}, time::Time};

use crate::{Ticks, commands::{ReversibleCommandInitialized, ReversibleCommand}, TicksRelative};

use super::consts::ControllerConsts;

/// `NonSend` resource containing sync channel `Receiver`s for forgets and delayed commands.
pub(super) struct ControllerReceivers {
    /// Messages about commands that are not happening in the next tick, is only sent to if progress is `ForwardFast`.
    commands: Receiver<(usize, Vec<Box<dyn ReversibleCommand>>)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProgressQuery{
    Forward,
    ForwardFast { to_time_stamp: Wrapping<Ticks> },
    ForwardLog,
    ForwardLogEnd,
    BackwardLog,
    BackwardLogEnd,
    Pause
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ProgressQueried{
    Forward,
    ForwardFast { 
        to_time_stamp: Wrapping<Ticks>,
        queried: Wrapping<Ticks> 
    },
    ForwardLog,
    ForwardLogEnd,
    BackwardLog,
    BackwardLogEnd,
    Pause
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum CurrentProgress{
    #[default]
    Forward,
    ForwardFast { init: bool },
    ForwardLog { after_backward: bool },
    ForwardLogEnd { after_backward_if_init: Option<bool> },
    BackwardLog { after_forward: bool },
    BackwardLogEnd { after_forward_if_init: Option<bool> },
    Pause { after_forward_if_log: Option<bool> },
    LogClose { after_forward: bool }
}

enum ProgressType{
    NotLog,
    ForwardLog,
    BackwardLog
}

pub(super) struct ProgressContainer{
    time_step_query: Option<f64>, //todo: use enum instead with options such as "back to default after n ticks"
    time_step: f64,
    elapsed: f64,
    time: Time,
    first_ran: bool,

    current: CurrentProgress,
    query: Option<ProgressQueried>,
    pub(super) time_stamp: Wrapping<Ticks>,
    pub(super) forget: Wrapping<Ticks>,
    pub(super) forward_fast_limit: Wrapping<Ticks>,

    log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
    log_index: usize,
    delayed_commands: VecDeque<Vec<Box<dyn ReversibleCommand>>>,
    commands_overflows: u64,

    #[allow(dead_code)]
    commands_sender: SyncSender<(usize, Vec<Box<dyn ReversibleCommand>>)>,
    #[cfg(debug_assertions)]
    debug: Vec<DebugLogContainer>,
    #[cfg(test)]
    constants: ControllerConsts,
}

#[cfg(debug_assertions)]
#[derive(Debug)]
struct DebugLogContainer{
    after_first: DebugLog,
    after_last: Option<DebugLog>
}

#[cfg(debug_assertions)]
#[derive(Debug)]
struct DebugLog{
    time_step_query: Option<f64>,
    time_step: f64,
    first_ran: bool,
    current: CurrentProgress,
    query: Option<ProgressQueried>,
    time_stamp: Wrapping<Ticks>,
    forget: Wrapping<Ticks>,
    forward_fast_limit: Wrapping<Ticks>,
    log_len: usize,
    log_index: usize,
    delayed_commands_len: usize,
    commands_overflows: u64,
}

#[cfg(debug_assertions)]
impl From<&ProgressContainer> for DebugLog{
    fn from(value: &ProgressContainer) -> Self {
        Self { 
            time_step_query: value.time_step_query, 
            time_step: value.time_step, 
            first_ran: value.first_ran, 
            current: value.current, 
            query: value.query, 
            time_stamp: value.time_stamp, 
            forget: value.forget, 
            forward_fast_limit: value.forward_fast_limit, 
            log_len: value.log.len(), 
            log_index: value.log_index ,
            delayed_commands_len: value.delayed_commands.len(),
            commands_overflows: value.commands_overflows
        }
    }
}

struct ContainerContainer{
    progress: ProgressContainer,
}

impl ProgressContainer{
    #[cfg(test)]
    pub(super) fn consts(&self) -> &ControllerConsts {
        &self.constants
    }
    #[cfg(not(test))]
    pub(super) const fn consts(&self) -> &ControllerConsts {
        use super::consts::CONTROLLER_CONSTS;
        &CONTROLLER_CONSTS
    }
    pub(super) fn log_start(&self) -> Wrapping<Ticks> {
        self.time_stamp - Wrapping(self.log.len() as Ticks)
    }
    pub(super) fn send_commands(
        &self,
        v: Vec<Box<dyn ReversibleCommand>>,
        from_now: Ticks,
        commands: &mut Commands<'_, '_>,
    ) {
        let index = from_now as usize;
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
    fn query(&mut self, query: ProgressQuery){
        self.query = Some(match query{
            ProgressQuery::Forward => ProgressQueried::Forward,
            ProgressQuery::ForwardFast { to_time_stamp } 
            => ProgressQueried::ForwardFast { to_time_stamp, queried: self.time_stamp },
            ProgressQuery::ForwardLog => ProgressQueried::ForwardLog,
            ProgressQuery::ForwardLogEnd => ProgressQueried::ForwardLogEnd,
            ProgressQuery::BackwardLog => ProgressQueried::BackwardLog,
            ProgressQuery::BackwardLogEnd => ProgressQueried::BackwardLogEnd,
            ProgressQuery::Pause => ProgressQueried::Pause
        });
    }
    fn first_forward_not_log(&mut self, commands: &mut Commands<'_, '_>){
        if self.log.len() == self.consts().log_len{
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
    fn first_backward(&mut self, commands: &mut Commands<'_, '_>){
        self.time_stamp -= 1;
        self.forget -= 1;
        if let Some(v) = self.log.get(self.log_index){
            if v.is_empty(){
                panic!("todo");
            }
            commands.add(|world: &mut World|{
                world.resource_scope(|world, mut controller: Mut<'_, Self>|{
                    let index = controller.log_index;
                    if let Some(entry) = controller.log.get_mut(index){
                        entry.iter_mut().for_each(|command| command.undo(world));
                    }
                    else {
                        panic!("todo");
                    }
                });
            });
        }
    }
    fn first_time(&mut self) -> bool{
        self.elapsed -= self.time.delta_seconds_f64();
        self.time.update();
        if self.elapsed > 0.0{
            return false;
        }
        self.elapsed += self.time_step;
        true
    }
    fn process_first(
        &mut self,
        commands: &mut Commands<'_, '_>, 
    ){
        match self.current {
            CurrentProgress::Forward => {
                if self.first_time(){
                    self.first_forward_not_log(commands);
                    self.first_ran = true;
                }
            },
            CurrentProgress::ForwardFast { .. } => {
                self.time.update();
                self.first_forward_not_log(commands);
                self.first_ran = true;
            },
            CurrentProgress::ForwardLog { after_backward } => {
                if self.first_time(){
                    self.first_ran = true;
                    if !after_backward{
                        self.log_index -= 1;
                    }
                }
            },
            CurrentProgress::ForwardLogEnd { after_backward_if_init } => {
                if after_backward_if_init != Some(true){
                    self.log_index -= 1;
                }
                self.first_ran = true;
            },
            CurrentProgress::BackwardLog { .. } => {
                if self.first_time(){
                    self.first_backward(commands);
                    self.first_ran = true;
                }
            },
            CurrentProgress::BackwardLogEnd { .. } => {
                self.first_backward(commands);
                self.first_ran = true;
            },
            CurrentProgress::Pause { .. } => {
                self.time.update();
                self.first_ran = true;
            },
            CurrentProgress::LogClose { .. } => {
                self.first_ran = true;
            },
        }
    }
    fn add_delayed_commands(
        &mut self,
        index: usize,
        mut commands: Vec<Box<dyn ReversibleCommand>>,
    ) {
        if let Some(v) = self.delayed_commands.get_mut(index){
            v.append(&mut commands);
        }
        else {
            panic!("todo");
        }
    }
    fn last_forward_commands(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceivers>,
        mut commands: Commands<'_, '_>,
        fast: bool,
    ) {
        if fast {
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
        } else {
            assert!(
                matches!(receivers.commands.try_recv(), Err(TryRecvError::Empty)),
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
            None => panic!(
                "todo",
            ),
        };
        if commands_exist {
            let log_index = self.log_index;
            commands.add(move |world: &mut World|{
                world.resource_scope(|world, mut controller: Mut<'_, Self>|{
                    let commands = match controller.log.get_mut(log_index){
                        Some(v) => v,
                        None => panic!("todo")
                    };
                    commands.iter_mut().for_each(|command| command.redo(world))
                })
            })
        }
    }
    fn apply_progress_query(&mut self, current: ProgressType) {
        /*
        log index 0, log len 1, after_forward:
        - forward_log nicht möglich
        - backward_log möglich

        log index 0, log len 1, after_backward:
        - forward_log möglich
        - backward_log nicht möglich

        forward_log:
        - log_index -= 1 bei first (kann nicht forward_log sein wen underflow passieren kann)
        - time_stamp += 1 bei last (skip bei after_backward)

        backward_log:
        - time_stamp -= 1 bei first (skip bei after_forward)
        - log_index += 1 bei last (wenn kein underflow)
        */
        if self.query.is_none(){
            return;
        }
        match (self.query.unwrap(), current){
            (ProgressQueried::Forward, ProgressType::NotLog) => {
                self.query = None;
                self.current = CurrentProgress::Forward;
            },
            (ProgressQueried::Forward, ProgressType::ForwardLog) => {
                self.current = CurrentProgress::LogClose { after_forward: true };
            },
            (ProgressQueried::Forward, ProgressType::BackwardLog) => {
                self.current = CurrentProgress::LogClose { after_forward: false };
            },
            (ProgressQueried::ForwardFast { 
                to_time_stamp, 
                queried 
            }, ProgressType::NotLog) => {
                self.query = None;
                if to_time_stamp == self.time_stamp + Wrapping(1){
                    self.current = CurrentProgress::Forward;
                }
                else if to_time_stamp.further_in_the_future(self.time_stamp, queried)
                {
                    let mut reserve = self.time_stamp.ticks_from_now(to_time_stamp) as usize;
                    if reserve == 0 {
                        reserve = Ticks::MAX as usize + 1;
                    }
                    let mut vd = VecDeque::from_iter((0..reserve).map(|_| Vec::new()));
                    self.delayed_commands.append(&mut vd);
                    self.forward_fast_limit = to_time_stamp;
                    self.current = CurrentProgress::ForwardFast { init: true };
                }
            },
            (ProgressQueried::ForwardFast { .. }, ProgressType::ForwardLog) => {
                self.current = CurrentProgress::LogClose { after_forward: true };
            },
            (ProgressQueried::ForwardFast { .. }, ProgressType::BackwardLog) => {
                self.current = CurrentProgress::LogClose { after_forward: false };
            },
            (ProgressQueried::ForwardLog, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = CurrentProgress::ForwardLog { after_backward: true };
            },
            (ProgressQueried::ForwardLog, _) => {
                self.query = None;
                if !self.log_index_min(){
                    self.current = CurrentProgress::ForwardLog { after_backward: false };
                }
            },
            (ProgressQueried::ForwardLogEnd, ProgressType::NotLog) => {
                self.query = None;
                self.current = match self.log_index{
                    0 => CurrentProgress::Pause { after_forward_if_log: None },
                    1 => CurrentProgress::ForwardLog { after_backward: false },
                    _ => CurrentProgress::ForwardLogEnd { after_backward_if_init: Some(false) }
                };
            },
            (ProgressQueried::ForwardLogEnd, ProgressType::ForwardLog) => {
                self.query = None;
                self.current = match self.log_index{
                    0 => CurrentProgress::Pause { after_forward_if_log: Some(true) },
                    1 => CurrentProgress::ForwardLog { after_backward: false },
                    _ => CurrentProgress::ForwardLogEnd { after_backward_if_init: Some(false) }
                };
            },
            (ProgressQueried::ForwardLogEnd, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = if self.log_index == 0{
                    CurrentProgress::ForwardLog { after_backward: true }
                }
                else {
                    CurrentProgress::ForwardLogEnd { after_backward_if_init: Some(true) }
                };
            },
            (ProgressQueried::BackwardLog, ProgressType::BackwardLog) => {
                self.query = None;
                if !self.log_index_max(){
                    self.current = CurrentProgress::BackwardLog { after_forward: false };
                }
            },
            (ProgressQueried::BackwardLog, _) => {
                self.query = None;
                self.current = CurrentProgress::BackwardLog { after_forward: true };
            },
            (ProgressQueried::BackwardLogEnd, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = match self.log.len() - self.log_index{
                    0 => CurrentProgress::Pause { after_forward_if_log: Some(false) },
                    1 => CurrentProgress::BackwardLog { after_forward: false },
                    _ => CurrentProgress::BackwardLogEnd { after_forward_if_init: Some(false) }
                };
            },
            (ProgressQueried::BackwardLogEnd, _) => {
                self.query = None;
                self.current = if self.log_index == 0{
                    CurrentProgress::BackwardLog{ after_forward: true }
                }
                else{
                    CurrentProgress::BackwardLogEnd{ after_forward_if_init: Some(true) }
                };
            },
            (ProgressQueried::Pause, ProgressType::NotLog) => {
                self.query = None;
                self.current = CurrentProgress::Pause { after_forward_if_log: None };
            },
            (ProgressQueried::Pause, ProgressType::ForwardLog) => {
                self.query = None;
                self.current = CurrentProgress::Pause { after_forward_if_log: Some(true) };
            },
            (ProgressQueried::Pause, ProgressType::BackwardLog) => {
                self.query = None;
                self.current = CurrentProgress::Pause { after_forward_if_log: Some(false) };
            }
        }
    }
    fn log_index_min(&self) -> bool{
        self.log_index == 0
    }
    fn log_index_max(&self) -> bool{
        self.log_index + 1 == self.log.len()
    }
    fn log_close_split(&mut self, mut commands: Commands<'_, '_>, after_forward: bool){
        let mut index = self.log_index;
        if after_forward{
            index += 1;
            if index == self.log.len(){
                return;
            }
        }
        //workaround for not having `VecDeque::split_off_front`, see https://github.com/rust-lang/rust/issues/92547
        let additional = self.log.len() - index;
        let mut split_off = self.log.split_off(index);
        split_off.reserve_exact(additional);
        std::mem::swap(&mut split_off, &mut self.log);
        //end of workaround
        self.log_index = 0;
        commands.add(move |world: &mut World| {
            split_off
                .into_iter()
                .flatten()
                .for_each(|command| command.undo_finalize(world))
        });
    }
    fn process_last(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceivers>,
        commands: Commands<'_, '_>
    ){
        let forward_stamps = |x: &mut Self|{
            x.time_stamp += 1;
            x.forget += 1;
        };
        self.first_ran = false;
        self.time_step = self.time_step_query.take().unwrap_or(self.time_step);
        match self.current {
            CurrentProgress::Forward => {
                self.last_forward_commands(receivers, commands, false);
                forward_stamps(self);
                self.apply_progress_query(ProgressType::NotLog);
            },
            CurrentProgress::ForwardFast { .. } => {
                self.last_forward_commands(receivers, commands, true);
                forward_stamps(self);
                if self.forward_fast_limit == self.time_stamp {
                    self.current = CurrentProgress::Pause { after_forward_if_log: None };
                    self.apply_progress_query(ProgressType::NotLog);
                }
                else {
                    self.current = CurrentProgress::ForwardFast { init: false };
                }
            },
            CurrentProgress::ForwardLog { .. } => {
                self.last_forward_commands_log(commands);
                forward_stamps(self);
                if self.log_index_min(){
                    self.current = CurrentProgress::Pause { after_forward_if_log: Some(true) };
                }
                self.apply_progress_query(ProgressType::ForwardLog);
            },
            CurrentProgress::ForwardLogEnd { .. } => {
                self.last_forward_commands_log(commands);
                forward_stamps(self);
                if self.log_index_min() {
                    self.current = CurrentProgress::Pause { after_forward_if_log: Some(true) };
                    self.apply_progress_query(ProgressType::ForwardLog);
                }
                else {
                    self.current = CurrentProgress::ForwardLogEnd { after_backward_if_init: None };
                }
            },
            CurrentProgress::BackwardLog { after_forward } => {
                if self.log_index_max(){
                    self.current = CurrentProgress::Pause { after_forward_if_log: Some(false) };
                }
                else if after_forward {
                    self.current = CurrentProgress::BackwardLog { after_forward: false };
                }
                else{
                    self.log_index += 1;
                }
                self.apply_progress_query(ProgressType::BackwardLog);
            },
            CurrentProgress::BackwardLogEnd { after_forward_if_init } => {
                let log_index = self.log_index + 1;
                if after_forward_if_init != Some(true) && log_index == self.log.len(){
                    self.current = CurrentProgress::Pause { after_forward_if_log: Some(false) };
                    self.apply_progress_query(ProgressType::BackwardLog);
                }
                else {
                    self.log_index = log_index;
                }
            },
            CurrentProgress::Pause { after_forward_if_log: None } => {
                self.apply_progress_query(ProgressType::NotLog);
            },
            CurrentProgress::Pause { after_forward_if_log: Some(false) } => {
                self.apply_progress_query(ProgressType::BackwardLog);
            },
            CurrentProgress::Pause { after_forward_if_log: Some(true) } => {
                self.apply_progress_query(ProgressType::ForwardLog);
            }
            CurrentProgress::LogClose { after_forward } => {
                self.log_close_split(commands, after_forward);
                self.current = CurrentProgress::Forward;
                self.apply_progress_query(ProgressType::NotLog)
            }
        }
    }
}



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
