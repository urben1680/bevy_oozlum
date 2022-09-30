use std::{collections::VecDeque, num::Wrapping, mem::swap, iter::FromIterator, sync::mpsc::{TryRecvError, Receiver, SyncSender, TrySendError, sync_channel}};

use bevy::{ecs::{schedule::ShouldRun, world::Mut}, prelude::{Commands, ResMut, Res, Events, World, Local, NonSendMut}, time::Time, log::info};

use crate::{commands::{ReversibleCommandInitialized, DelayedCommand}, Ticks, FORGET_SYNC_SENDER_CAPACITY, DEFAULT_TIME_STEP, MAX_LOG_LEN, DELAYED_COMMANDS_SYNC_SENDER_CAPACITY, DELAYED_COMMANDS_TICKS_CAPACITY};

/// Event to forget all logs inclusively to the inner value.
pub(super) struct Forget(pub(super) Wrapping<Ticks>);

impl Forget{
    pub(super) fn send(self, commands: &mut Commands){
        //~~todo: solve this with `EventWriter<Forget>` when it can be used in `query.par_for_each_mut` https://github.com/bevyengine/bevy/discussions/6093~~
        //todo: same event writers hinter parallelization, so keep commands or mpsc channels
        //todo: Forget inner value can be normalized deltas because they are applied in the same tick anyway
        commands.add(|world: &mut World|{
            world.resource_mut::<Events<Forget>>().send(self);
        })
    }
}

pub(super) struct ControllerReceivers{
    /// Messages about forgetting n ticks in the past, is only sent to if progress is `Forward`or `ForwardFast`.
    forget: Receiver<Ticks>,
    /// Messages about commands that are not happening in the next tick, is only sent to if progress is `ForwardFast`.
    delayed_commands: Receiver<(usize, Box<dyn DelayedCommand>)>
}

/// Send this event to control the length of the time steps.
/// Raising this value speeds the progression up while being more expensive.
/// Lowering this value slows the progression down while being less expensive.
/// 
/// If fast-forward is desired, prefer sending a `Progress::ForwardFast{ to_time_stamp }` event instead.
/// This approach should not be significantly more expensive.
/// See `Progress` for more information.
pub struct TimeStep(pub f64);

/// Used to control the behavior of reversible systems.
/// 
/// - `Forward` progresses all systems step-by-step in sync.
/// - `ForwardFast { to_time_stamp }` progresses some systems eagerly until `to_time_stamp` is reached. 
/// This cannot be aborted because affected systems are not in sync until the end. 
/// However, it can be extended at any point by setting this variant again with a larger `to_time_stamp` relatively to `time_stamp()`.
/// If `to_time_stamp` was reached and still `FastForward` is queried, the progression is changed to `Forward`.
/// - `ForwardLog` progresses all systems using each system's log(s) step-by-step. 
/// If this is attempted while the log reached it's most recent end, the progression is changed to `PauseLog`. 
/// - `ForwardLogEnd` progresses to the most recent log end, potentially cheaper than with `ForwardLog`.
/// This cannot be aborted because affected systems are not in sync until the end.
/// - `BackwardLog` reverts all systems using each system's log(s) step-by-step.
/// - `BackwardLogEnd` reverts all systems to the most past log end, potentially cheaper than with `BackwardLog`.
/// This cannot be aborted because affected systems are not in sync until the end.
/// - `Pause` halts everything until another non-pause variant is picked.
/// - `PauseLog` behaves like `Pause` but does not cause logs in the future to be forgotten.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Progress{
    Forward,
    ForwardFast{
        to_time_stamp: Wrapping<Ticks>
    },
    ForwardLog,
    ForwardLogEnd, //calls same systems as ForwardFast
    BackwardLog,
    BackwardLogEnd,
    Pause,
    PauseLog,
}

impl Progress{
    /// Returns `true` if `self` is `ForwardLog`, `ForwardLogEnd`, `BackwardLog` or `BackwardLogEnd`.
    /// 
    /// If parameter `including_pause` is set to true, the above check includes `PauseLog`.
    /// 
    /// Otherwise returns `false`.
    pub fn is_log(&self, including_pause: bool) -> bool{
        matches!(self, Progress::ForwardLog |
            Progress::ForwardLogEnd |
            Progress::BackwardLog |
            Progress::BackwardLogEnd)
        || (including_pause && self == &Progress::PauseLog)
    }
    /// Returns `true` if `self` is `Pause`.
    /// 
    /// If parameter `including_log` is set to true, the above check includes `PauseLog`.
    /// 
    /// Otherwise returns `false`.
    pub fn is_pause(&self, including_log: bool) -> bool{
        self == &Progress::Pause || (including_log && self == &Progress::PauseLog)
    }

    /// Returns `true` if `self` in `Controller` causes progress in fixed time steps.
    /// 
    /// Returns `false` if `self` in `Controller` causes progress as fast as possible.
    pub fn is_fixed_time_step(&self) -> bool{
        matches!(self, Progress::Forward |
            Progress::ForwardLog |
            Progress::BackwardLog
        )
    }
    /// Returns `true` if `self` is `Forward` or `ForwardFast`.
    /// 
    /// Otherwise returns `false`.
    pub fn is_not_log_nor_pause(&self) -> bool{
        matches!(self, Progress::Forward | Progress::ForwardFast { .. })
    }
}

pub struct Controller{
    progress_query: Option<Progress>, 
    time_step_query: Option<f64>,
    time_step: f64,
    /// Log containing commands to undo or redo them. 
    /// 
    /// 1. After the stage `Last`, during `Forward` and `ForwardFast`, old commands may be split off and finalized.
    /// 2. After the stage `Last`, during `LogEnd`, new commands may be split off and finalized.
    /// 
    /// `VecDeque` has no `split_off_front` method: https://github.com/rust-lang/rust/issues/92547.
    /// The workaround uses needless allocations. So this should be done in the less common case: 2.
    /// Because of this the front of the `VecDeque` is the most recent end and the back the most past end of the log.
    log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
    //pub(super) next_entry: Vec<Box<dyn ReversibleCommandInitialized>>,
    time_stamp: Wrapping<Ticks>,
    /// Current log index, todo: reverse now that deque ends are reversed!!
    log_index: usize,
    progress: Progress,
    //progress_query: Progress,
    pre_update_ran: bool,
    log_end: bool,
    delayed_commands_sender: SyncSender<(usize, Box<dyn DelayedCommand>)>,
    delayed_commands: VecDeque<Vec<Box<dyn DelayedCommand>>>,
    delayed_commands_overflows: u64,
    forget_sender: SyncSender<Ticks>,
    forget_overflow: Option<Ticks>, 
    forget_overflows: u64,
}

impl Controller{
    /// Get the time stamp at which a change of the progress is possible again.
    pub fn target_time_stamp(&self) -> Wrapping<Ticks>{
        match self.progress{
            Progress::Forward | Progress::ForwardLog => self.time_stamp + Wrapping(1),
            Progress::ForwardFast { to_time_stamp } => to_time_stamp,
            Progress::ForwardLogEnd => self.time_stamp + Wrapping((self.log.len() - self.log_index - 1) as u16), //todo log_index reversed
            Progress::BackwardLog => self.time_stamp - Wrapping(1),
            Progress::BackwardLogEnd => self.time_stamp - Wrapping(self.log_index as Ticks), //todo log_index reversed
            Progress::Pause | Progress::PauseLog => self.time_stamp
        }
    }
    /// Get the current time stamp.
    pub fn time_stamp(&self) -> Wrapping<Ticks>{
        self.time_stamp
    }
    /// Get the time stamp to which everything can be reverted to.
    pub fn age(&self) -> Ticks{
        (self.time_stamp - Wrapping(self.log.len() as u16)).0 + 1 //correct?
    }
    pub fn log_front(&self) -> bool{
        self.log_index == 0
    }
    pub fn log_back(&self) -> bool{
        self.log_index + 1 == self.log.len()
    }
    pub fn set_time_step(&mut self, time_step: f64){
        self.time_step_query = Some(time_step);
    }
    pub fn time_step(&self) -> Option<f64>{
        if self.progress.is_fixed_time_step(){
            Some(self.time_step)
        } else {
            None
        }
    }
    pub fn set_progress(&mut self, progress: Progress){
        self.progress_query = Some(progress);
    }
    pub fn progress(&self) -> Progress{
        self.progress
    }
    /// Add new command.
    pub(super) fn send_command<T: ReversibleCommandInitialized, Marker>(&mut self, command: T){
        debug_assert!(matches!(self.progress, Progress::Forward | Progress::ForwardFast { .. }));
        self
            .log
            .front_mut()
            .expect("`log` should not be empty")
            .push(Box::new(command));
    }
    pub(super) fn send_forget(&self, time_stamp: Wrapping<Ticks>, commands: &mut Commands){
        debug_assert_eq!(self.progress, Progress::Forward);
        let ticks = (time_stamp - self.time_stamp).0;
        if ticks > self.age(){
            return;
        }
        match self.forget_sender.try_send((time_stamp - self.time_stamp).0){
            Ok(()) => return,
            Err(TrySendError::Full(a)) => {
                commands.add(move |world: &mut World|{
                    let mut controller = world.resource_mut::<Self>();
                    controller.forget_overflows += 1;
                    if !matches!(controller.forget_overflow, Some(b) if b < a){
                        controller.forget_overflow = Some(a);
                    }
                })
            },
            Err(TrySendError::Disconnected(_)) => {
                panic!("Could not send forget timestamp, receiver disconnected.");
            }
        }
    }
    pub(super) fn send_delayed_command<T: DelayedCommand>(&self, time_stamp: Wrapping<Ticks>, command: T, commands: &mut Commands){
        debug_assert!(matches!(self.progress, Progress::ForwardFast { .. }));
        let index = (time_stamp - self.time_stamp).0 as usize;
        match self.delayed_commands_sender.try_send((index, Box::new(command))){
            Ok(_) => {},
            Err(TrySendError::Full((index, command))) => {
                commands.add(move |world: &mut World|{
                    let mut controller = world.resource_mut::<Self>();
                    controller.delayed_commands_overflows += 1;
                    controller.add_delayed_command(index, command);
                })
            },
            Err(TrySendError::Disconnected(_)) => {
                panic!("Could not send delayed command, receiver disconnected.");
            }
        }
    }
    fn add_delayed_command(&mut self, index: usize, command: Box<dyn DelayedCommand>){
        match self.delayed_commands.get_mut(index){
            Some(v) => v.push(command),
            None => panic!(
                "`delayed_commands` with `len` {} is too short for `index` {}, `len` should be {}.",
                self.log.len(), index, (self.target_time_stamp() - self.time_stamp).0
            )
        }
    }
    fn apply_progress_query(&mut self){
        if let Some(progress) = self.progress_query.take(){
            self.log_end = self.progress.is_log(true) && !progress.is_log(true);
            self.progress = progress;
        }
    }
}

/// Run criterias for other systems
impl Controller{
    pub fn run_criteria<const LOG: bool, const PAUSE: bool>(controller: Res<Self>) -> ShouldRun{
        Self::should_run(match (LOG, PAUSE){
            (false, false) => controller.progress.is_not_log_nor_pause(),
            (false, true) => controller.progress.is_pause(false),
            (true, false) => controller.progress.is_log(false),
            (true, true) => controller.progress.is_log(true)
        })
    }
    pub fn run_criteria_forward(controller: Res<Self>) -> ShouldRun{
        controller.after_pre_update_not_log_end(controller.progress == Progress::Forward)
    }
    pub fn run_criteria_forward_fast(controller: Res<Self>) -> ShouldRun{
        controller.after_pre_update_not_log_end(matches!(controller.progress, Progress::ForwardFast { .. }))
    }
    pub fn run_criteria_forward_log(controller: Res<Self>) -> ShouldRun{
        controller.after_pre_update_not_log_end(controller.progress == Progress::ForwardLog)
    }
    pub fn run_criteria_forward_log_fast(controller: Res<Self>) -> ShouldRun{
        controller.after_pre_update_not_log_end(controller.progress == Progress::ForwardLogEnd)
    }
    pub fn run_criteria_backward_log(controller: Res<Self>) -> ShouldRun{
        controller.after_pre_update_not_log_end(controller.progress == Progress::BackwardLog)
    }
    pub fn run_criteria_backward_log_fast(controller: Res<Self>) -> ShouldRun{
        controller.after_pre_update_not_log_end(controller.progress == Progress::BackwardLogEnd)
    }
    pub fn run_criteria_log_end(controller: Res<Self>) -> ShouldRun{
        controller.after_pre_update(controller.log_end)
    }
    fn after_pre_update_not_log_end(&self, b: bool) -> ShouldRun{
        self.after_pre_update(b && !self.log_end)
    }
    fn after_pre_update(&self, b: bool) -> ShouldRun{
        Self::should_run(b && self.pre_update_ran)
    }
    fn should_run(b: bool) -> ShouldRun{
        if b{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
}

/// Systems and commands
impl Controller{
    /// Command to load `Controller` and `ControllerReceivers`.
    pub(super) fn insert_command(log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>, time_stamp: Wrapping<Ticks>) -> impl FnOnce(&mut World){
        move |world: &mut World|{
            let (forget_s, forget_r) = sync_channel(FORGET_SYNC_SENDER_CAPACITY);
            let (commands_s, commands_r) = sync_channel(DELAYED_COMMANDS_SYNC_SENDER_CAPACITY);
            world.insert_non_send_resource(ControllerReceivers{
                forget: forget_r,
                delayed_commands: commands_r
            });
            world.insert_resource(Self{
                time_step_query: None,
                progress_query: None,
                time_step: DEFAULT_TIME_STEP,
                log,
                time_stamp,
                log_index: 0,
                progress: Progress::Forward,
                pre_update_ran: false,
                log_end: false,
                delayed_commands_sender: commands_s,
                delayed_commands: VecDeque::with_capacity(DELAYED_COMMANDS_TICKS_CAPACITY),
                delayed_commands_overflows: 0,
                forget_sender: forget_s,
                forget_overflow: None,
                forget_overflows: 0
            })
        }
    }
    /// System that should be run before all reversible systems.
    pub (super) fn first_system(mut controller: ResMut<Self>, time: Local<Time>, elapsed: Local<f64>, mut commands: Commands){
        if !controller.first_early_return(time, elapsed){
            controller.first_update(commands);
        }
    }
    /// System that should be run after all reversible systems.
    pub(super) fn last_system(mut controller: ResMut<Self>, receivers: NonSendMut<ControllerReceivers>, commands: Commands){
        if controller.pre_update_ran{
            controller.last(receivers, commands);
        }
    }
    fn first_early_return(&mut self, mut time: Local<Time>, mut elapsed: Local<f64>) -> bool{
        if self.log_end{
            self.pre_update_ran = true;
            return true;
        } 
        match self.progress{
            Progress::ForwardFast { to_time_stamp: _ } |
            Progress::ForwardLogEnd |
            Progress::BackwardLogEnd |
            Progress::Pause |
            Progress::PauseLog => {
                time.update();
            }
            Progress::Forward |
            Progress::ForwardLog |
            Progress::BackwardLog => {
                *elapsed -= time.delta_seconds_f64();
                time.update();
                if *elapsed > 0.0{
                    return true;
                }
                *elapsed += self.time_step;
                self.pre_update_ran = true;
            }
        }
        false
    }
    fn first_update(&mut self, mut commands: Commands){
        match self.progress{
            Progress::Forward | Progress::ForwardFast{to_time_stamp: _} => {
                self.time_stamp += 1;
                if self.log.len() == MAX_LOG_LEN{
                    let forget = self.log.pop_back().expect("`MAX_LOG_LEN` should not be 0.");
                    if !forget.is_empty(){
                        commands.add(|world: &mut World|{
                            forget.into_iter().for_each(|mut command| command.redo_finalize(world))
                        })
                    }
                } else {
                    self.log.push_front(Default::default());
                }
            },
            Progress::ForwardLog | Progress::ForwardLogEnd => {
                self.log_index -= 1;
                self.time_stamp += 1;
            },
            Progress::BackwardLog | Progress::BackwardLogEnd => {
                match self.log.get(self.log_index){
                    Some(v) => {
                        if !v.is_empty(){
                            commands.add(|world: &mut World|{
                                world.resource_scope(|world, mut controller: Mut<Self>|{
                                    let log_index = controller.log_index;
                                    match controller.log.get_mut(log_index){
                                        Some(entry) => {
                                            entry.iter_mut().for_each(|command|command.undo(&mut *world));
                                        },
                                        None => {
                                            panic!("`log` with `len` {} is too short for `log_index` {} in `first_update` method command", controller.log.len(), controller.log_index);
                                        }
                                    }
                                });
                            });
                        }
                    },
                    None => {
                        panic!("`log` with `len` {} is too short for `log_index` {} in `first_update` method", self.log.len(), self.log_index);
                    }
                }
            },
            Progress::Pause | Progress::PauseLog => {}
        }
    }
    fn last(&mut self, receivers: NonSendMut<ControllerReceivers>, commands: Commands){
        self.pre_update_ran = false;
        if let Some(time_step) = self.time_step_query.take(){
            self.time_step = time_step;
        }
        if self.log_end{
            self.log_end(commands);
            return;
        }
        self.last_progress_query_check();
        self.last_progress_check(receivers, commands);
    }
    fn forward_commands(&mut self, receivers: NonSendMut<ControllerReceivers>, mut commands: Commands, fast_forward: bool){
        let delayed = if fast_forward{
            receivers
                .delayed_commands
                .try_iter()
                .for_each(|(index, command)| self.add_delayed_command(index, command));
            if self.delayed_commands_overflows != 0{
                info!("`delayed_commands_overflows` {} with DELAYED_COMMANDS_SYNC_SENDER_CAPACITY {}", 
                    self.delayed_commands_overflows, DELAYED_COMMANDS_SYNC_SENDER_CAPACITY);
                self.delayed_commands_overflows = 0;
            }
            self.delayed_commands.pop_front().expect("`delayed_commands` should not be empty before `FastForward` is finished")
        } else {
            debug_assert!(matches!(receivers.delayed_commands.try_recv(), Err(TryRecvError::Empty)));
            debug_assert_eq!(self.delayed_commands_overflows, 0);
            Default::default()
        };

        let forget = receivers
            .forget
            .iter()
            .chain(self.forget_overflow.take())
            .min()
            .map(|ticks| self.log.split_off(self.log.len() - ticks as usize))
            .filter(|vd| vd.iter().any(|v| !v.is_empty()));

        if delayed.is_empty() && forget.is_none(){
            debug_assert_eq!(self.forget_overflows, 0);
            return;
        }

        if self.forget_overflows != 0{
            info!("`forget_overflows` {} with FORGET_SYNC_SENDER_CAPACITY {}", 
                self.forget_overflows, FORGET_SYNC_SENDER_CAPACITY);
            self.forget_overflows = 0;
        }

        commands.add(|world: &mut World|{
            unsafe{
                //SAFETY: calls `ManuallyDrop::take` which is only allowed to be done once, which is the case here before `command` is dropped
                delayed.into_iter().for_each(|mut command| command.init(world));
            }
            forget
                .into_iter()
                .flatten()
                .flatten()
                .for_each(|mut command| command.redo_finalize(world));
        })
    }
    fn forward_log_commands(&self, mut commands: Commands){
        let commands_exist = match self.log.get(self.log_index){
            Some(v) => !v.is_empty(),
            None => panic!("`log` with `len` {} is too short for `log_index` {} during `ForwardLog` system.", self.log.len(), self.log_index)
        };
        if commands_exist{
            let log_len = self.log.len();
            let log_index = self.log_index;
            commands.add(move |world: &mut World|{
                world.resource_scope(|world, mut controller: Mut<Self>|{
                    let commands = match controller.log.get_mut(log_index){
                        Some(v) => v,
                        None => panic!("`log` with `len` {} is too short for `log_index` {} during `ForwardLog` command.", log_len, log_index)
                    };
                    commands.iter_mut().for_each(|command| command.redo(world))
                })
            })
        }
    }
    fn log_end(&mut self, mut commands: Commands){
        self.log_end = false;
        if self.log_front(){
            return;
        }
        //workaround for not having `VecDeque::split_off_front`, see https://github.com/rust-lang/rust/issues/92547
        let additional = self.log.capacity() - self.log_index;
        let mut split_off = self.log.split_off(self.log_index);
        split_off.reserve_exact(additional);
        swap(&mut split_off, &mut self.log);
        //end of workaround
        self.log_index = 0;
        commands.add(move |world: &mut World|{
            split_off
                .into_iter()
                .flatten()
                .for_each(|mut command|command.undo_finalize(world))
        });
    }
    fn last_progress_query_check(&mut self){
        if let Some(progress_query) = self.progress_query{
            match progress_query{
                Progress::ForwardFast { to_time_stamp } => {
                    if progress_query != self.progress{
                        let mut delta = (to_time_stamp - self.time_stamp).0 as usize;
                        delta = delta
                            .checked_sub(self.delayed_commands.len())
                            .expect("`delayed_commands.len()` should be less than or equal target ticks");
                        let mut vd = VecDeque::from_iter((0..delta).map(|_|Vec::new()));
                        self.delayed_commands.append(&mut vd);
                    }
                },
                Progress::ForwardLog | Progress::ForwardLogEnd => {
                    if self.log_front(){
                        self.progress_query = None;
                    }
                },
                Progress::BackwardLog | Progress::BackwardLogEnd => {
                    if self.log_back(){
                        self.progress_query = None;
                    }
                },
                _ => {}
            }
        }
    }
    fn last_progress_check(&mut self, receivers: NonSendMut<ControllerReceivers>, commands: Commands){
        match &mut self.progress{
            Progress::Forward => {
                self.apply_progress_query();
                self.forward_commands(receivers, commands, false);
            },
            Progress::ForwardFast { to_time_stamp } => {
                if let Some(Progress::ForwardFast { to_time_stamp: update }) = self.progress_query{
                    if update - self.time_stamp > *to_time_stamp - self.time_stamp{
                        *to_time_stamp = update;
                    }
                }
                if *to_time_stamp == self.time_stamp{
                    self.apply_progress_query();
                }
                self.forward_commands(receivers, commands, true);
            },
            Progress::ForwardLog => {
                self.forward_log_commands(commands);
                self.apply_progress_query();
            },
            Progress::ForwardLogEnd => {
                self.forward_log_commands(commands);
                if self.log_front(){
                    self.apply_progress_query();
                }
            },
            Progress::BackwardLog => {
                self.time_stamp -= 1;
                self.log_index += 1;
                self.apply_progress_query();
            },
            Progress::BackwardLogEnd => {
                if self.log_back(){
                    self.apply_progress_query();
                }
            }
            Progress::PauseLog | Progress::Pause => {
                self.apply_progress_query();
            }
        }
    }
}