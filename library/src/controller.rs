use std::{collections::VecDeque, num::Wrapping};

use bevy::{ecs::schedule::ShouldRun, prelude::{Commands, ResMut, Res, Events}, time::Time};

use crate::commands::ReversibleCommand;

/// Event to forget all logs inclusively to `Some(time_stamp)`. `None` signals forgetting all logs.
pub(super) struct Forget(pub Option<Wrapping<u16>>);

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
        to_time_stamp: Wrapping<u16>
    },
    ForwardLog,
    ForwardLogEnd,
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
    pub(super) log: VecDeque<Vec<Box<dyn ReversibleCommand>>>,
    time_stamp: Wrapping<u16>,
    log_index: usize,
    progress: Progress,
    progress_query: Progress,
    elapsed: f64,
    pre_update_ran: bool,
    log_end: bool,
}

impl Controller{
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
    pub fn time_stamp(&self) -> Wrapping<u16>{
        self.time_stamp
    }
    pub fn remembers_back_to(&self) -> Wrapping<u16>{
        self.time_stamp - Wrapping(self.log.len() as u16) + Wrapping(1) //correct?
    }
    pub fn progress(&self) -> Progress{
        self.progress
    }
    fn forget(vec: Option<Vec<Box<dyn ReversibleCommand>>>, commands: &mut Commands){
        vec.into_iter().flatten().for_each(|mut entry| entry.cleanup(commands));
    }
    pub (super) fn system_pre_update(mut controller: ResMut<Self>, mut time: ResMut<Time>, mut commands: Commands){
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
                controller.log.push_back(Vec::new());
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
    pub (super) fn system_post_update(mut controller: ResMut<Self>, mut commands: Commands, events: ResMut<Events<Forget>>, mut progress_query: ResMut<Events<Progress>>, mut time_step: ResMut<Events<ControllerTimeStep>>)
    {
        if !controller.pre_update_ran{
            return; //do not run system
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
                Self::post_update_forward(&mut controller, commands, events);
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
                Self::post_update_forward(&mut controller, commands, events);
                if *a != controller.time_stamp{
                    return;
                }
                controller.progress_query = Progress::Forward;
            },
            (Progress::ForwardFast{to_time_stamp}, _) => {
                Self::post_update_forward(&mut controller, commands, events);
                if *to_time_stamp != controller.time_stamp{
                    return;
                }
            },
            (Progress::ForwardLog, query) => {
                let log_index = controller.log_index;
                for entry in controller.log.get_mut(log_index).unwrap(){
                    entry.redo(&mut commands);
                }
                controller.log_end = !query.log();
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
            (Progress::BackwardLog, query) => {
                controller.time_stamp -= 1;
                controller.log_index -= 1;
                controller.log_end = !query.log();
            },
            (Progress::BackwardLogEnd, _) => {
                if controller.log_index != 0{
                    return;
                }
            }
            (Progress::PauseLog, query) => {
                controller.log_end = !query.log();
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
            Progress::Forward | 
            Progress::ForwardFast { to_time_stamp: _ } | 
            Progress::Pause | 
            Progress::PauseLog => {}
        }

        controller.progress = controller.progress_query;
    }
    fn post_update_forward(controller: &mut ResMut<Self>, mut commands: Commands, mut events: ResMut<Events<Forget>>){
        let count = events
            .drain()
            .map(|time_stamp| {
                time_stamp.0.map_or(usize::MAX, |value| (value - controller.time_stamp).0 as usize)
            })
            .max();
        if let Some(mut count) = count{
            count = count.min(controller.log_index); //should controller.log_index be replaced with controller.log.len()?
            for _ in 0..count{
                Self::forget(controller.log.pop_front(), &mut commands);
            }
            controller.log_index = controller.log.len() - 1;
        }
    }
}