use std::{collections::{VecDeque, BTreeMap}, num::Wrapping, mem::take, iter::FromIterator};

use bevy::{ecs::schedule::ShouldRun, prelude::{Commands, ResMut, Res, Events, World}, time::Time};

use crate::{commands::{ReversibleCommandInitialized, DelayedCommand}, Ticks};

/// Event to forget all logs inclusively to `Some(time_stamp)`. `None` signals forgetting all logs.
pub(super) struct Forget(pub(super) Option<Wrapping<Ticks>>); //is it always Some?

impl Forget{
    pub(super) fn send(self, commands: &mut Commands){
        commands.add(|world: &mut World|{
            let mut controller = world.resource_mut::<Controller>();
            controller.forget_buffer = match &controller.forget_buffer{
                None => Some(self),
                Some(old) => {
                    match (old.0, self.0){
                        (Some(mut a), Some(mut b)) => {
                            a -= controller.time_stamp;
                            b -= controller.time_stamp;
                            let c = a.min(b) + controller.time_stamp;
                            Some(Forget(Some(c)))
                        },
                        _ => Some(Forget(None))
                    }
                }
            }
        })
    }
}

/// Send this event to control the length of the time steps.
/// Raising this value speeds the progression up while being more expensive.
/// Lowering this value slows the progression down while being less expensive.
/// 
/// If fast-forward is desired, prefer sending a `Progress::ForwardFast{ to_time_stamp }` event instead.
/// This approach should not be significantly more expensive.
/// See `Progress` for more information.
pub struct ControllerTimeStep(pub f64);

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
#[derive(Clone, Copy)]
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
    pub fn log(&self) -> bool{
        matches!(self, Progress::ForwardLog |
            Progress::ForwardLogEnd |
            Progress::BackwardLog |
            Progress::BackwardLogEnd |
            Progress::PauseLog)
    }
}

pub struct Controller{
    time_step: f64,
    log: VecDeque<Vec<Box<dyn ReversibleCommandInitialized>>>,
    pub(super) next_entry: Vec<Box<dyn ReversibleCommandInitialized>>,
    pub(super) time_stamp: Wrapping<Ticks>,
    log_index: usize,
    progress: Progress,
    progress_query: Progress,
    elapsed: f64,
    pre_update_ran: bool,
    log_end: bool,
    forget_buffer: Option<Forget>,
    pub(super) delayed_commands: VecDeque<Vec<Box<dyn DelayedCommand>>>
}

impl Controller{
    pub fn target_time_stamp(&self) -> Wrapping<Ticks>{
        match self.progress{
            Progress::Forward | Progress::ForwardLog => self.time_stamp + Wrapping(1),
            Progress::ForwardFast { to_time_stamp } => to_time_stamp,
            Progress::ForwardLogEnd => self.time_stamp + Wrapping((self.log.len() - self.log_index - 1) as u16),
            Progress::BackwardLog => self.time_stamp - Wrapping(1),
            Progress::BackwardLogEnd => self.time_stamp - Wrapping(self.log_index as Ticks),
            Progress::Pause | Progress::PauseLog => self.time_stamp
        }
    }
    pub (super) fn run_criteria_forward(controller: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(controller.progress, Progress::Forward), controller)
    }
    pub (super) fn run_criteria_forward_fast(controller: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(controller.progress, Progress::ForwardFast{to_time_stamp: _}), controller)
    }
    pub (super) fn run_criteria_forward_log(controller: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(controller.progress, Progress::ForwardLog), controller)
    }
    pub (super) fn run_criteria_forward_log_fast(controller: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(controller.progress, Progress::ForwardLogEnd), controller)
    }
    pub (super) fn run_criteria_backward_log(controller: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(controller.progress, Progress::BackwardLog), controller)
    }
    pub (super) fn run_criteria_backward_log_fast(controller: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(controller.progress, Progress::BackwardLogEnd), controller)
    }
    pub (super) fn run_criteria_log_end(controller: Res<Self>) -> ShouldRun{
        if controller.log_end && controller.pre_update_ran{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    pub (super) fn run_criteria_pause(controller: Res<Self>) -> ShouldRun{
        if let Progress::Pause = controller.progress{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    pub (super) fn run_criteria_pause_log(controller: Res<Self>) -> ShouldRun{
        if let Progress::PauseLog = controller.progress{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_criteria_not_log_end(check: bool, controller: Res<Self>) -> ShouldRun{
        if !controller.log_end && controller.pre_update_ran && check{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    pub fn time_stamp(&self) -> Wrapping<Ticks>{
        self.time_stamp
    }
    pub fn age(&self) -> Wrapping<Ticks>{
        self.time_stamp - Wrapping(self.log.len() as u16) + Wrapping(1) //correct?
    }
    pub fn progress(&self) -> Progress{
        self.progress
    }
    fn forget(vec: Option<Vec<Box<dyn ReversibleCommandInitialized>>>, commands: &mut Commands){
        vec.into_iter().flatten().for_each(|mut entry| entry.cleanup(commands));
    }
    pub (super) fn system_pre_reversible_systems(mut controller: ResMut<Self>, mut time: ResMut<Time>, mut commands: Commands){
        if controller.log_end{
            controller.pre_update_ran = true;
            return; //nothing to do for this state
        } 
        match controller.progress{
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
                controller.elapsed -= time.delta_seconds_f64();
                time.update();
                if controller.elapsed > 0.0{
                    return; //do not run system
                }
                controller.elapsed += controller.time_step;
                controller.pre_update_ran = true;
            }
        }
        match controller.progress{
            Progress::Forward | Progress::ForwardFast{to_time_stamp: _} => {
                if controller.log.len() == controller.log.capacity(){
                    Self::forget(controller.log.pop_front(), &mut commands);
                }
                else{
                    controller.log_index += 1;
                }
                let next = take(&mut controller.next_entry);
                controller.log.push_back(next);
                controller.time_stamp += 1;
            },
            Progress::ForwardLog | Progress::ForwardLogEnd => {
                controller.log_index += 1;
                controller.time_stamp += 1;
            },
            Progress::BackwardLog | Progress::BackwardLogEnd => {
                let log_index = controller.log_index;
                for entry in controller.log.get_mut(log_index).unwrap(){
                    entry.undo(&mut commands);
                }
            },
            Progress::Pause | Progress::PauseLog => {}
        }
    }
    pub (super) fn system_post_reversible_systems(mut controller: ResMut<Self>, mut commands: Commands, mut progress_query: ResMut<Events<Progress>>, mut time_step: ResMut<Events<ControllerTimeStep>>)
    {
        if !controller.pre_update_ran{
            return;
        }
        
        controller.pre_update_ran = false;

        if let Some(progress_query) = progress_query.drain().last(){
            controller.progress_query = progress_query;
        }
        if let Some(time_step) = time_step.drain().last(){
            controller.time_step = time_step.0;
        }

        if controller.log_end{
            let target = controller.log_index + 1;
            while controller.log.len() > target{
                Self::forget(controller.log.pop_back(), &mut commands);
            }
            controller.log_end = false;
            return;
        }

        match &mut (controller.progress, controller.progress_query){
            (Progress::Forward, _) => {
                Self::post_update_forward(&mut controller, commands, false);
            },
            (
                Progress::ForwardFast{to_time_stamp: a}, 
                Progress::ForwardFast{to_time_stamp: b}
            ) => {
                if a != b{
                    let a_delta = *a - controller.time_stamp;
                    let b_delta = *b - controller.time_stamp;
                    if a_delta < b_delta{
                        *a = *b;
                    } else {
                        *b = *a;
                    }
                }
                Self::post_update_forward(&mut controller, commands, true);
                if *a != controller.time_stamp{
                    return;
                }
                controller.progress_query = Progress::Forward;
                debug_assert!(controller.delayed_commands.is_empty());
            },
            (Progress::ForwardFast{to_time_stamp}, _) => {
                Self::post_update_forward(&mut controller, commands, true);
                if *to_time_stamp != controller.time_stamp{
                    return;
                }
                debug_assert!(controller.delayed_commands.is_empty());
            },
            (Progress::ForwardLog, _) => {
                let log_index = controller.log_index;
                for entry in controller.log.get_mut(log_index).unwrap(){
                    entry.redo(&mut commands);
                }
                controller.log_end = !controller.progress_query.log();
            },
            (Progress::ForwardLogEnd, _) => {
                let log_index = controller.log_index;
                for entry in controller.log.get_mut(log_index).unwrap(){
                    entry.redo(&mut commands);
                }
                if controller.log_index + 1 != controller.log.len(){
                    return;
                }
            },
            (Progress::BackwardLog, _) => {
                controller.time_stamp -= 1;
                controller.log_index -= 1;
                controller.log_end = !controller.progress_query.log();
            },
            (Progress::BackwardLogEnd, _) => {
                if controller.log_index != 0{
                    return;
                }
            }
            (Progress::PauseLog, _) => {
                controller.log_end = !controller.progress_query.log();
            },
            (Progress::Pause, _) => {}
        }

        match controller.progress_query{
            Progress::ForwardLog | Progress::ForwardLogEnd => {
                if controller.log_index + 1 == controller.log.len(){
                    controller.progress_query = Progress::PauseLog;
                }
            },
            Progress::BackwardLog | Progress::BackwardLogEnd => {
                if controller.log_index == 0{
                    controller.progress_query = Progress::PauseLog;
                }
            }, 
            Progress::ForwardFast { to_time_stamp } => {
                let mut delta = (to_time_stamp - controller.time_stamp).0 as usize;
                delta = delta
                    .checked_sub(controller.delayed_commands.len())
                    .expect("`delayed_commands.len()` should be less than target ticks");
                if delta != 0{
                    let mut vd = VecDeque::from_iter((0..delta).map(|_|Vec::new()));
                    controller.delayed_commands.append(&mut vd);
                }
            },
            Progress::Forward |
            Progress::Pause | 
            Progress::PauseLog => {}
        }

        controller.progress = controller.progress_query;
    }
    fn post_update_forward(controller: &mut ResMut<Self>, mut commands: Commands, to_time_stamp: bool){
        if let Some(forget) = controller.forget_buffer.take(){
            let mut count = controller.log.len();
            if let Some(forget) = forget.0{
                let delta = controller.time_stamp - forget;
                count = count.min(delta.0 as usize);
            }
            for _ in 0..count{
                Self::forget(controller.log.pop_front(), &mut commands);
            }
            controller.log_index = controller.log.len() - 1;
        }
        if to_time_stamp{
            controller
                .delayed_commands
                .pop_front()
                .expect("`delayed_commands` should not be empty before FastForward is finished")
                .into_iter()
                .for_each(|mut command|{
                unsafe{
                    //SAFETY: calls `ManuallyDrop::take` which is only allowed to be done once, which is the case here before `command` is dropped
                    command.init(&mut commands);
                }
            });
        }
    }
}