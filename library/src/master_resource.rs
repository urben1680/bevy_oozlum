use std::{collections::VecDeque, num::Wrapping};

use bevy::{ecs::{system::Resource, schedule::ShouldRun}, prelude::{Commands, ResMut, Res, Events}, time::Time};

/// Event to forget all logs inclusively to `Some(time_stamp)`. `None` signals forgetting all logs.
pub struct ForgetEvent(pub Option<Wrapping<u16>>);

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

pub(super) trait MasterEntryTrait: Resource{
    fn forward(&self, commands: &mut Commands);
    fn backward(&self, commands: &mut Commands);
    fn forget(&self, commands: &mut Commands);
}

pub struct Master{
    /// Change this property to control the next progression of all reversible systems
    pub progress_query: Progress,
    /// Change this property to control the length of the time steps.
    /// Raising this value speeds the progression up while being more expensive.
    /// Lowering this value slows the progression down while being less expensive.
    /// 
    /// If fast-forward is desired, prefer setting `progress_query = Progress::ForwardFast{ to_time_stamp }`.
    /// This approach should not be significantly more expensive.
    /// See `Progress` for more information.
    pub time_step: f64,
    pub(super) log: VecDeque<Vec<Box<dyn MasterEntryTrait>>>,
    time_stamp: Wrapping<u16>,
    log_index: usize,
    progress: Progress,
    elapsed: f64,
    pre_update_ran: bool,
    log_end: bool,
}

impl Master{
    pub (super) fn run_criteria_forward(master: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(master.progress, Progress::Forward), master)
    }
    pub (super) fn run_criteria_forward_fast(master: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(master.progress, Progress::ForwardFast{to_time_stamp: _}), master)
    }
    pub (super) fn run_criteria_forward_log(master: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(master.progress, Progress::ForwardLog), master)
    }
    pub (super) fn run_criteria_forward_log_fast(master: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(master.progress, Progress::ForwardLogEnd), master)
    }
    pub (super) fn run_criteria_backward_log(master: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(master.progress, Progress::BackwardLog), master)
    }
    pub (super) fn run_criteria_backward_log_fast(master: Res<Self>) -> ShouldRun{
        Self::run_criteria_not_log_end(matches!(master.progress, Progress::BackwardLogEnd), master)
    }
    pub (super) fn run_criteria_log_end(master: Res<Self>) -> ShouldRun{
        if master.log_end && master.pre_update_ran{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    pub (super) fn run_criteria_pause(master: Res<Self>) -> ShouldRun{
        if let Progress::Pause = master.progress{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    pub (super) fn run_criteria_pause_log(master: Res<Self>) -> ShouldRun{
        if let Progress::PauseLog = master.progress{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_criteria_not_log_end(check: bool, master: Res<Self>) -> ShouldRun{
        if !master.log_end && master.pre_update_ran && check{
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
    fn forget(vec: Option<Vec<Box<dyn MasterEntryTrait>>>, commands: &mut Commands){
        vec.into_iter().flatten().for_each(|entry| entry.forget(commands));
    }
    pub (super) fn system_pre_update(mut master: ResMut<Self>, mut time: ResMut<Time>, mut commands: Commands){
        if master.log_end{
            master.pre_update_ran = true;
            return; //nothing to do for this state
        } 
        match master.progress{
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
                master.elapsed -= time.delta_seconds_f64();
                time.update();
                if master.elapsed > 0.0{
                    return; //do not run system
                }
                master.elapsed += master.time_step;
                master.pre_update_ran = true;
            }
        }
        match master.progress{
            Progress::Forward | Progress::ForwardFast{to_time_stamp: _} => {
                if master.log.len() == master.log.capacity(){
                    Self::forget(master.log.pop_front(), &mut commands);
                }
                else{
                    master.log_index += 1;
                }
                master.log.push_back(Vec::new());
                master.time_stamp += 1;
            },
            Progress::ForwardLog | Progress::ForwardLogEnd => {
                master.log_index += 1;
                master.time_stamp += 1;
            },
            Progress::BackwardLog | Progress::BackwardLogEnd => {
                for entry in master.log.get(master.log_index).unwrap(){
                    entry.backward(&mut commands);
                }
            },
            Progress::Pause | Progress::PauseLog => {}
        }
    }
    pub (super) fn system_post_update(mut master: ResMut<Self>, mut commands: Commands, events: ResMut<Events<ForgetEvent>>)
    {
        if !master.pre_update_ran{
            return; //do not run system
        }
        master.pre_update_ran = false;

        if master.log_end{
            let target = master.log_index + 1;
            while master.log.len() > target{
                Self::forget(master.log.pop_back(), &mut commands);
            }
            master.log_end = false;
            return;
        }

        match &mut (master.progress, master.progress_query){
            (Progress::Forward, _) => {
                Self::post_update_forward(&mut master, commands, events);
            },
            (Progress::ForwardFast{to_time_stamp: a}, Progress::ForwardFast{to_time_stamp: b}) => {
                if a != b{
                    let a_delta = *a - master.time_stamp;
                    let b_delta = *b - master.time_stamp;
                    if a_delta < b_delta{
                        *a = *b;
                    } else {
                        *b = *a;
                    }
                }
                Self::post_update_forward(&mut master, commands, events);
                if *a != master.time_stamp{
                    return;
                }
                master.progress_query = Progress::Forward;
            },
            (Progress::ForwardFast{to_time_stamp}, _) => {
                Self::post_update_forward(&mut master, commands, events);
                if *to_time_stamp != master.time_stamp{
                    return;
                }
            },
            (Progress::ForwardLog, query) => {
                for entry in master.log.get(master.log_index).unwrap(){
                    entry.forward(&mut commands);
                }
                master.log_end = !query.log();
            },
            (Progress::ForwardLogEnd, _) => {
                for entry in master.log.get(master.log_index).unwrap(){
                    entry.forward(&mut commands);
                }
                if master.log_index + 1 != master.log.len(){
                    return;
                }
            },
            (Progress::BackwardLog, query) => {
                master.time_stamp -= 1;
                master.log_index -= 1;
                master.log_end = !query.log();
            },
            (Progress::BackwardLogEnd, _) => {
                if master.log_index != 0{
                    return;
                }
            }
            (Progress::PauseLog, query) => {
                master.log_end = !query.log();
            },
            (Progress::Pause, _) => {}
        }

        match master.progress_query{
            Progress::ForwardLog | Progress::ForwardLogEnd => {
                if master.log_index + 1 == master.log.len(){
                    master.progress_query = Progress::PauseLog;
                }
            },
            Progress::BackwardLog | Progress::BackwardLogEnd => {
                if master.log_index == 0{
                    master.progress_query = Progress::PauseLog;
                }
            },
            Progress::Forward | 
            Progress::ForwardFast { to_time_stamp: _ } | 
            Progress::Pause | 
            Progress::PauseLog => {}
        }

        master.progress = master.progress_query;
    }
    fn post_update_forward(master: &mut ResMut<Self>, mut commands: Commands, mut events: ResMut<Events<ForgetEvent>>){
        let count = events
            .drain()
            .map(|time_stamp| {
                time_stamp.0.map_or(usize::MAX, |value| (value - master.time_stamp).0 as usize)
            })
            .max();
        if let Some(mut count) = count{
            count = count.min(master.log_index); //should master.log_index be replaced with master.log.len()?
            for _ in 0..count{
                Self::forget(master.log.pop_front(), &mut commands);
            }
            master.log_index = master.log.len() - 1;
        }
    }
}