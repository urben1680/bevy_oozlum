use std::{collections::VecDeque, num::Wrapping};

use bevy::{ecs::{system::Resource, schedule::ShouldRun}, prelude::{Commands, ResMut, Res, Events}, time::Time};

pub struct ForgetEvent(pub Option<Wrapping<u16>>);

#[derive(Clone, Copy)]
pub enum Progress{
    Forward,
    ForwardFast{
        to_time_stamp: Wrapping<u16>
    },
    ForwardLog{
        forward_at_end: bool,
    },
    BackwardLog,
    //LogEnd, //user should set Forward, ForwardFast or Pause instead
    Pause,
    PauseLog,
}

impl Progress{
    fn log(&self) -> bool{
        match self{
            Progress::ForwardLog{forward_at_end: _} |
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
    progress: Progress,
    pub time_step: f64,
    elapsed: f64,
    pre_update_ran: bool,
    log_end: bool,
}

impl Master{
    pub (super) const RUN_CRITERIA_PAUSE: for<'a> fn(Res<'a, Master>) -> ShouldRun = Self::run_pause;

    pub (super) const RUN_CRITERIA_PRE_UPDATE_FORWARD: for<'a> fn(ResMut<'a, Master>, ResMut<'a, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_forward_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_FORWARD_FAST: for<'a> fn(ResMut<'a, Master>, ResMut<'a, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_forward_fast_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_FORWARD_LOG: for<'a> fn(ResMut<'a, Master>, ResMut<'a, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_forward_log_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_BACKWARD_LOG: for<'a> fn(ResMut<'a, Master>, ResMut<'a, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_backward_log_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_LOG_END: for<'a> fn(ResMut<'a, Master>) -> ShouldRun = Self::run_log_end_pre_update;

    pub (super) const RUN_CRITERIA_UPDATE_FORWARD: for<'a> fn(Res<'a, Master>) -> ShouldRun = Self::run_forward_update;
    pub (super) const RUN_CRITERIA_UPDATE_FORWARD_FAST: for<'a> fn(Res<'a, Master>) -> ShouldRun = Self::run_forward_fast_update;
    pub (super) const RUN_CRITERIA_UPDATE_FORWARD_LOG: for<'a> fn(Res<'a, Master>) -> ShouldRun = Self::run_forward_log_update;
    pub (super) const RUN_CRITERIA_UPDATE_BACKWARD_LOG: for<'a> fn(Res<'a, Master>) -> ShouldRun = Self::run_backward_log_update;
    pub (super) const RUN_CRITERIA_UPDATE_LOG_END: for<'a> fn(Res<'a, Master>) -> ShouldRun = Self::run_log_end_update;

    pub (super) const RUN_CRITERIA_POST_UPDATE_FORWARD: for<'a> fn(ResMut<'a, Master>) -> ShouldRun = Self::run_forward_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_FORWARD_FAST: for<'a> fn(ResMut<'a, Master>) -> ShouldRun = Self::run_forward_fast_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_FORWARD_LOG: for<'a> fn(ResMut<'a, Master>) -> ShouldRun = Self::run_forward_log_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_BACKWARD_LOG: for<'a> fn(ResMut<'a, Master>) -> ShouldRun = Self::run_backward_log_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_LOG_END: for<'a> fn(ResMut<'a, Master>) -> ShouldRun = Self::run_log_end_post_update;

    pub (super) const SYSTEM_PRE_UPDATE: for<'a, 'w, 's> fn(ResMut<'a, Master>, Commands<'w, 's>) = Self::system_pre_update;
    pub (super) const SYSTEM_POST_UPDATE: for<'a, 'w, 's> fn(ResMut<'a, Master>, Commands<'w, 's>, ResMut<'a, Events<ForgetEvent>>) = Self::system_post_update;


    fn log_present(&self) -> bool{
        self.log_index + 1 == self.log.len()
    }
    fn log_most_distant_past(&self) -> bool{
        self.log_index == 0
    }
    pub fn query_progress(&mut self, progress: Progress){
        self.log_end = self.progress.log() && !progress.log();
        self.progress_query = progress;
    }
    fn run_pre_update(check: bool, mut master: ResMut<Self>, mut time: ResMut<Time>) -> ShouldRun{
        if !master.log_end && check{
            master.elapsed -= time.delta_seconds_f64();
            time.update();
            if master.elapsed <= 0.0{
                master.elapsed += master.time_step;
                master.pre_update_ran = true;
                return ShouldRun::Yes
            }
        }
        ShouldRun::No
    }
    fn run_update_not_log_end(check: bool, master: Res<Self>) -> ShouldRun{
        if !master.log_end && master.pre_update_ran && check{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_post_update(check: bool, mut master: ResMut<Self>) -> ShouldRun{
        if check && master.pre_update_ran{
            master.pre_update_ran = false;
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_forward_pre_update(master: ResMut<Self>, time: ResMut<Time>) -> ShouldRun{
        Self::run_pre_update(matches!(master.progress, Progress::Forward), master, time)
    }
    fn run_forward_fast_pre_update(master: ResMut<Self>, time: ResMut<Time>) -> ShouldRun{
        Self::run_pre_update(matches!(master.progress, Progress::ForwardFast{to_time_stamp: _}), master, time)
    }
    fn run_forward_log_pre_update(master: ResMut<Self>, time: ResMut<Time>) -> ShouldRun{
        Self::run_pre_update(matches!(master.progress, Progress::ForwardLog{forward_at_end: _}), master, time)
    }
    fn run_backward_log_pre_update(master: ResMut<Self>, time: ResMut<Time>) -> ShouldRun{
        Self::run_pre_update(matches!(master.progress, Progress::BackwardLog), master, time)
    }
    fn run_log_end_pre_update(mut master: ResMut<Self>) -> ShouldRun{
        if master.log_end{
            master.pre_update_ran = true;
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_forward_update(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::Forward), master)
    }
    fn run_forward_fast_update(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::ForwardFast{to_time_stamp: _}), master)
    }
    fn run_forward_log_update(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::ForwardLog{forward_at_end: _}), master)
    }
    fn run_backward_log_update(master: Res<Self>) -> ShouldRun{
        Self::run_update_not_log_end(matches!(master.progress, Progress::BackwardLog), master)
    }
    fn run_log_end_update(master: Res<Self>) -> ShouldRun{
        if master.log_end && master.pre_update_ran{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_forward_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::Forward), master)
    }
    fn run_forward_fast_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::ForwardFast{to_time_stamp: _}), master)
    }
    fn run_forward_log_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::ForwardLog{forward_at_end: _}), master)
    }
    fn run_backward_log_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::BackwardLog), master)
    }
    fn run_log_end_post_update(mut master: ResMut<Self>) -> ShouldRun{
        if master.log_end && master.pre_update_ran{
            master.pre_update_ran = false;
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_pause(master: Res<Self>) -> ShouldRun{
        if let Progress::Pause = master.progress{
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
    fn check_log_end(&mut self){
        match self.progress_query{
            Progress::ForwardLog{forward_at_end} => {
                if self.log_present(){
                    if forward_at_end{
                        self.log_end = true;
                        self.progress_query = Progress::Forward;
                    } else {
                        self.progress_query = Progress::PauseLog;
                    }
                }
                self.progress = self.progress_query;
            },
            Progress::BackwardLog => {
                if self.log_most_distant_past(){
                    self.progress_query = Progress::PauseLog;
                }
                self.progress = self.progress_query;
            },
            Progress::PauseLog => {
                self.progress = self.progress_query;
            },
            _ => {
                self.log_end = true;
                self.progress = self.progress_query;
            }
        }
    }
    fn forget(vec: Option<Vec<Box<dyn MasterEntryTrait>>>, commands: &mut Commands){
        vec.into_iter().flatten().for_each(|entry| entry.forget(commands));
    }
    fn system_pre_update(mut master: ResMut<Self>, mut commands: Commands){
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
            Progress::ForwardLog{forward_at_end: _} => {
                master.log_index += 1;
                master.time_stamp += 1;
            },
            Progress::BackwardLog => {
                for entry in master.log.get(master.log_index).unwrap(){
                    entry.backward(&mut commands);
                }
                master.check_log_end();
            },
            Progress::Pause | Progress::PauseLog => {}
        }
    }
    fn system_post_update(mut master: ResMut<Self>, mut commands: Commands, events: ResMut<Events<ForgetEvent>>)
    {
        if master.log_end{
            let target = master.log_index + 1;
            while master.log.len() > target{
                Self::forget(master.log.pop_back(), &mut commands);
            }
            master.progress = master.progress_query;
            master.log_end = false;
            return;
        }
        match &mut (master.progress, master.progress_query){
            (Progress::Forward, _) => {
                Self::post_update_forward(&mut master, commands, events);
                master.progress = master.progress_query;
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
                if *a == master.time_stamp{
                    master.progress = master.progress_query;
                }
            },
            (Progress::ForwardFast{to_time_stamp}, _) => {
                Self::post_update_forward(&mut master, commands, events);
                if *to_time_stamp == master.time_stamp{
                    master.progress = master.progress_query;
                }
            },
            (Progress::ForwardLog{forward_at_end: _}, _) => {
                for entry in master.log.get(master.log_index).unwrap(){
                    entry.forward(&mut commands);
                }
                master.check_log_end();
            },
            (Progress::BackwardLog, _) => {
                master.time_stamp -= 1;
                master.log_index -= 1;
            },
            (Progress::Pause, _) | (Progress::PauseLog, _) => {
                master.progress = master.progress_query;
            }
        }
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

