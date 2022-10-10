use std::{
    collections::VecDeque,
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
    Ticks, TicksRelative, DEFAULT_TIME_STEP, DELAYED_COMMANDS_SYNC_SENDER_CAPACITY,
    DELAYED_COMMANDS_TICKS_CAPACITY, LOG_LEN,
};

/// `NonSend` resource containing sync channel `Receiver`s for forgets and delayed commands.
pub(super) struct ControllerReceivers {
    /// Messages about commands that are not happening in the next tick, is only sent to if progress is `ForwardFast`.
    commands: Receiver<(usize, Vec<Box<dyn ReversibleCommand>>)>,
}

/// `Progress` is used to control the progression of all reversible systems.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Progress {
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

pub struct Controller {
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
    fast_init: bool, //todo make systems
    log_end: bool,
    commands_sender: SyncSender<(usize, Vec<Box<dyn ReversibleCommand>>)>,
    delayed_commands: VecDeque<Vec<Box<dyn ReversibleCommand>>>,
    commands_overflows: u64,
    //forget_sender: SyncSender<Ticks>,
    //forget_overflow: Option<Ticks>,
    //forget_overflows: u64,
}

impl Controller {
    /// Get the time stamp at which a change of the progress is possible again.
    pub fn target_time_stamp(&self) -> Wrapping<Ticks> {
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
    /// Get the time stamp to which everything can be reverted to.
    pub fn age(&self) -> Ticks {
        self.ticks_ago(Wrapping(self.log.len() as Ticks))
    }
    pub fn log_start(&self) -> Wrapping<Ticks> {
        self.time_stamp - Wrapping(self.log.len() as Ticks)
    }
    pub fn log_front(&self) -> bool {
        self.log_index == 0
    }
    pub fn log_back(&self) -> bool {
        self.log_index + 1 == self.log.len()
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
    pub fn query_progress(&mut self, progress: Progress) {
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
    /*
    pub(super) fn send_forget(&self, time_stamp: Wrapping<Ticks>, commands: &mut Commands<'_, '_>) {
        debug_assert_eq!(self.progress, Progress::Forward);
        let ticks = self.ticks_ago(time_stamp);
        if ticks > self.age() {
            return;
        }
        match self.forget_sender.try_send(ticks) {
            Ok(_) => return,
            Err(TrySendError::Full(ticks)) => commands.add(move |world: &mut World| {
                let mut controller = world.resource_mut::<Self>();
                controller.forget_overflows += 1;
                if !matches!(controller.forget_overflow, Some(other) if other < ticks) {
                    controller.forget_overflow = Some(ticks);
                }
            }),
            Err(TrySendError::Disconnected(_)) => {
                panic!("Could not send forget timestamp, receiver disconnected.")
            }
        }
    }
    */
    pub(super) fn send_commands(
        &self,
        v: Vec<Box<dyn ReversibleCommand>>,
        commands: &mut Commands<'_, '_>,
    ) {
        debug_assert_eq!(
            self.progress,
            Progress::Forward,
            "`send_commands` should not be called during `progress`: `{:?}`",
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
        debug_assert!(
            matches!(self.progress, Progress::ForwardFast { .. }),
            "`send_commands` should not be called during `progress`: `{:?}`",
            self.progress
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
        mut commands: I,
    ) {
        debug_assert!(matches!(
            self.progress,
            Progress::Forward | Progress::ForwardFast { .. }
        ));
        self.log
            .front_mut()
            .expect("`log` should not be empty")
            .extend(commands);
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
                "`delayed_commands` with `len` {} is too short for `index` {}, `len` should be {}.",
                self.log.len(), index, self.ticks_from_now(self.target_time_stamp())
            )
            }
        }
    }
    fn apply_progress_query(&mut self, or: Progress) {
        let query = self.progress_query.take().unwrap_or(or);
        self.log_end = self.progress.is_log(true) && !query.is_log(true);
        self.fast_init = !self.progress.is_fast() && query.is_fast();
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
        Self::should_run(
            match (LOG, PAUSE, LOG_PAUSE) {
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
            } || (LOG_END && controller.log_end),
        )
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
        Self::should_run(b && self.pre_update_ran)
    }
    fn should_run(b: bool) -> ShouldRun {
        if b {
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
}

/// Systems and commands
impl Controller {
    /// Command to load `Controller` and `ControllerReceivers`.
    pub(super) fn insert_command(
        log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
        time_stamp: Wrapping<Ticks>,
    ) -> impl FnOnce(&mut World) {
        move |world: &mut World| {
            //let (forget_s, forget_r) = sync_channel(FORGET_SYNC_SENDER_CAPACITY);
            let (commands_s, commands_r) = sync_channel(DELAYED_COMMANDS_SYNC_SENDER_CAPACITY);
            world.insert_non_send_resource(ControllerReceivers {
                //forget: forget_r,
                commands: commands_r,
            });
            world.insert_resource(Self {
                time_step_query: None,
                progress_query: None,
                time_step: DEFAULT_TIME_STEP,
                log,
                time_stamp,
                log_index: 0,
                progress: Progress::Forward,
                pre_update_ran: false,
                log_end: false,   //log_end systems must have been run before save
                fast_init: false, //cannot occure just before `Forward`
                commands_sender: commands_s,
                delayed_commands: VecDeque::with_capacity(DELAYED_COMMANDS_TICKS_CAPACITY),
                commands_overflows: 0,
                //forget_sender: forget_s,
                //forget_overflow: None,
                //forget_overflows: 0,
            })
        }
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
            Progress::ForwardFast { .. }
            | Progress::ForwardLogEnd
            | Progress::BackwardLogEnd
            | Progress::Pause
            | Progress::PauseLog => {
                time.update();
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
        }
        false
    }
    fn first_update(&mut self, mut commands: Commands<'_, '_>) {
        match self.progress {
            Progress::Forward | Progress::ForwardFast { .. } => {
                self.time_stamp += 1;
                if self.log.len() == LOG_LEN {
                    let forget = self.log.pop_back().expect("`MAX_LOG_LEN` should not be 0.");
                    if !forget.is_empty() {
                        commands.add(|world: &mut World| {
                            forget
                                .into_iter()
                                .for_each(|mut command| command.redo_finalize(world))
                        })
                    }
                } else {
                    self.log.push_front(Default::default());
                }
            }
            Progress::ForwardLog | Progress::ForwardLogEnd => {
                self.log_index -= 1;
                self.time_stamp += 1;
            }
            Progress::BackwardLog | Progress::BackwardLogEnd => {
                match self.log.get(self.log_index) {
                    Some(v) => {
                        if !v.is_empty() {
                            commands.add(|world: &mut World|{
                                world.resource_scope(|world, mut controller: Mut<'_, Self>|{
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
                    }
                    None => {
                        panic!("`log` with `len` {} is too short for `log_index` {} in `first_update` method", self.log.len(), self.log_index);
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
        self.validate_progress_query();
        self.progress_check(receivers, commands);
    }
    fn forward_commands(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceivers>,
        mut commands: Commands<'_, '_>,
        fast_forward: bool,
    ) {
        let delayed = if fast_forward {
            receivers
                .commands
                .try_iter()
                .for_each(|(index, command)| self.add_delayed_commands(index, command));
            if self.commands_overflows != 0 {
                info!(
                    "`delayed_commands_overflows` {} with DELAYED_COMMANDS_SYNC_SENDER_CAPACITY {}",
                    self.commands_overflows, DELAYED_COMMANDS_SYNC_SENDER_CAPACITY
                );
                self.commands_overflows = 0;
            }
            self.delayed_commands
                .pop_front()
                .expect("`delayed_commands` should not be empty during `FastForward`.")
        } else {
            debug_assert!(matches!(
                receivers.commands.try_recv(),
                Err(TryRecvError::Empty)
            ));
            debug_assert_eq!(self.commands_overflows, 0);
            self.delayed_commands
                .pop_front()
                .unwrap_or_else(|| Default::default())
        };
        /*
                let forget = receivers
                    .forget
                    .try_iter()
                    .chain(self.forget_overflow.take())
                    .min()
                    .map(|ticks| self.log.split_off(self.log.len() - ticks as usize))
                    .filter(|vd| vd.iter().any(|v| !v.is_empty()));
        */
        if delayed.is_empty()
        /*&& forget.is_none()*/
        {
            //debug_assert_eq!(self.forget_overflows, 0);
            return;
        }
        /*
            if self.forget_overflows != 0 {
                info!(
                    "`forget_overflows` {} with FORGET_SYNC_SENDER_CAPACITY {}",
                    self.forget_overflows, FORGET_SYNC_SENDER_CAPACITY
                );
                self.forget_overflows = 0;
            }
        */

        commands.add(|world: &mut World| {
            delayed.into_iter().for_each(|mut command| unsafe {
                //SAFETY: calls `ManuallyDrop::take` which is only allowed to be done once, which is the case here before `command` is dropped
                command.init(world);
            });
            /*
            forget
                .into_iter()
                .flatten()
                .flatten()
                .for_each(|mut command| command.redo_finalize(world));
                */
        })
    }
    fn forward_log_commands(&self, mut commands: Commands<'_, '_>) {
        let commands_exist = match self.log.get(self.log_index) {
            Some(v) => !v.is_empty(),
            None => panic!(
                "`log` with `len` {} is too short for `log_index` {} during `ForwardLog` system.",
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
                        None => panic!("`log` with `len` {} is too short for `log_index` {} during `ForwardLog` command.", log_len, log_index)
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
        if self.log_front() {
            return;
        }
        //workaround for not having `VecDeque::split_off_front`, see https://github.com/rust-lang/rust/issues/92547
        let additional = self.log.len() - self.log_index;
        let mut split_off = self.log.split_off(self.log_index);
        split_off.reserve_exact(additional);
        swap(&mut split_off, &mut self.log);
        //end of workaround
        self.log_index = 0;
        commands.add(move |world: &mut World| {
            split_off
                .into_iter()
                .flatten()
                .for_each(|mut command| command.undo_finalize(world))
        });
    }
    fn validate_progress_query(&mut self) {
        if let Some(query) = self.progress_query {
            match query {
                Progress::ForwardFast { to_time_stamp } => {
                    if let Progress::ForwardFast {
                        to_time_stamp: previous,
                    } = &mut self.progress
                    {
                        if to_time_stamp.further_in_the_future(*previous, self.time_stamp) {
                            *previous = to_time_stamp;
                        } else {
                            self.progress_query = None;
                            return;
                        }
                    }
                    let mut delta = self.ticks_from_now(to_time_stamp) as usize;
                    delta = delta.checked_sub(self.delayed_commands.len()).expect(
                        "`delayed_commands.len()` should be less than or equal target ticks",
                    );
                    let mut vd = VecDeque::from_iter((0..delta).map(|_| Vec::new()));
                    self.delayed_commands.append(&mut vd);
                }
                Progress::ForwardLog | Progress::ForwardLogEnd => {
                    if self.log_front() {
                        self.progress_query = None;
                    }
                }
                Progress::BackwardLog | Progress::BackwardLogEnd => {
                    if self.log_back() {
                        self.progress_query = None;
                    }
                }
                _ => {}
            }
        }
    }
    fn progress_check(
        &mut self,
        receivers: NonSendMut<'_, ControllerReceivers>,
        commands: Commands<'_, '_>,
    ) {
        match &mut self.progress {
            Progress::Forward => {
                self.apply_progress_query(self.progress);
                self.forward_commands(receivers, commands, false);
            }
            Progress::ForwardFast { to_time_stamp } => {
                if *to_time_stamp == self.time_stamp {
                    self.apply_progress_query(Progress::Forward);
                }
                self.forward_commands(receivers, commands, true);
            }
            Progress::ForwardLog => {
                self.forward_log_commands(commands);
                self.apply_progress_query(Progress::PauseLog);
            }
            Progress::ForwardLogEnd => {
                self.forward_log_commands(commands);
                if self.log_front() {
                    self.apply_progress_query(Progress::PauseLog);
                }
            }
            Progress::BackwardLog => {
                self.time_stamp -= 1;
                self.log_index += 1;
                self.apply_progress_query(Progress::PauseLog);
            }
            Progress::BackwardLogEnd => {
                self.time_stamp -= 1;
                self.log_index += 1;
                if self.log_back() {
                    self.apply_progress_query(Progress::PauseLog);
                }
            }
            Progress::PauseLog | Progress::Pause => {
                self.apply_progress_query(self.progress);
            }
        }
    }
}
