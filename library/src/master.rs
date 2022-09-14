use std::{collections::VecDeque, num::Wrapping};

use bevy::{ecs::{system::Resource, schedule::ShouldRun}, prelude::{Commands, ResMut, Res, Events}, time::Time};

pub struct ForgetEvent(pub Option<Wrapping<u16>>);

#[derive(Clone, Copy)]
pub enum Progress{
    Forward,
    ForwardFast{
        to_time_stamp: Wrapping<u16>
    },
    ForwardLog,
    BackwardLog,
    Pause,
    PauseLog,
}

impl Progress{
    fn log(&self) -> bool{
        match self{
            Progress::ForwardLog |
            Progress::BackwardLog |
            Progress::PauseLog => true,
            _ => false
        }
    }
}

pub trait MasterEntryTrait: Resource{
    fn forward(&self, commands: &mut Commands);
    fn backward(&self, commands: &mut Commands);
    fn forget(&self, commands: &mut Commands);
}

pub struct Master{
    pub(super) log: VecDeque<Vec<Box<dyn MasterEntryTrait>>>,
    time_stamp: Wrapping<u16>,
    log_index: usize,
    pub progress_query: Progress,
    progress: Progress, //gets ignored if log_end is true
    pub time_step: f64,
    elapsed: f64,
    pre_update_ran: bool,
    log_end: bool,
}

impl Master{
    fn run_update_not_log_end(check: bool, master: Res<Self>) -> ShouldRun{
        if !master.log_end && master.pre_update_ran && check{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    pub (super) fn run_criteria_forward(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::Forward), master)
    }
    pub (super) fn run_criteria_forward_fast(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::ForwardFast{to_time_stamp: _}), master)
    }
    pub (super) fn run_criteria_forward_log(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::ForwardLog), master)
    }
    pub (super) fn run_criteria_backward_log(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::BackwardLog), master)
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
    pub fn time_stamp(&self) -> Wrapping<u16>{
        self.time_stamp
    }
    pub fn progress(&self) -> Progress{
        self.progress
    }
    fn forget(vec: Option<Vec<Box<dyn MasterEntryTrait>>>, commands: &mut Commands){
        vec.into_iter().flatten().for_each(|entry| entry.forget(commands));
    }
    pub (super) fn system_pre_update(mut master: ResMut<Self>, mut time: ResMut<Time>, mut commands: Commands){
        master.elapsed -= time.delta_seconds_f64();
        time.update();
        if master.elapsed > 0.0{
            return; //do not run system
        }
        master.elapsed += master.time_step;
        master.pre_update_ran = true;

        if master.log_end{
            return;
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
            Progress::ForwardLog => {
                master.log_index += 1;
                master.time_stamp += 1;
            },
            Progress::BackwardLog => {
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
            (Progress::BackwardLog, query) => {
                master.time_stamp -= 1;
                master.log_index -= 1;
                master.log_end = !query.log();
            },
            (Progress::PauseLog, query) => {
                master.log_end = !query.log();
            }
            (Progress::Pause, _) => {}
        }
        match master.progress_query{
            Progress::ForwardLog if master.log_index + 1 == master.log.len() => {
                master.progress_query = Progress::PauseLog;
            }
            Progress::BackwardLog if master.log_index == 0 => {
                master.progress_query = Progress::PauseLog;
            }
            _ => {}
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

