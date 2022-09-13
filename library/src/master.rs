use std::{collections::VecDeque, num::Wrapping};

use bevy::{ecs::{system::Resource, schedule::ShouldRun}, prelude::{Commands, ResMut, Res, Events}, time::Time};

pub struct ForgetEvent(pub Option<Wrapping<u16>>);

#[derive(Clone, Copy)]
pub enum Progress{
    Forward,
    ForwardFast{
        steps: u16
    },
    ForwardLog{
        forward_at_end: bool,
    },
    BackwardLog,
    LogEnd,
    Pause,
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
}

impl Master{
    pub (super) const RUN_CRITERIA_PAUSE: for<'r> fn(Res<'r, Master>) -> ShouldRun = Self::run_pause;

    pub (super) const RUN_CRITERIA_PRE_UPDATE_FORWARD: for<'r, 's> fn(ResMut<'r, Master>, ResMut<'s, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_forward_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_FORWARD_FAST: for<'r, 's> fn(ResMut<'r, Master>, ResMut<'s, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_forward_fast_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_FORWARD_LOG: for<'r, 's> fn(ResMut<'r, Master>, ResMut<'s, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_forward_log_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_BACKWARD_LOG: for<'r, 's> fn(ResMut<'r, Master>, ResMut<'s, Time>) -> bevy::ecs::schedule::ShouldRun = Self::run_backward_log_pre_update;
    pub (super) const RUN_CRITERIA_PRE_UPDATE_LOG_END: for<'r> fn(ResMut<'r, Master>) -> ShouldRun = Self::run_log_end_pre_update;

    pub (super) const RUN_CRITERIA_UPDATE_FORWARD: for<'r> fn(Res<'r, Master>) -> ShouldRun = Self::run_forward_update;
    pub (super) const RUN_CRITERIA_UPDATE_FORWARD_FAST: for<'r> fn(Res<'r, Master>) -> ShouldRun = Self::run_forward_fast_update;
    pub (super) const RUN_CRITERIA_UPDATE_FORWARD_LOG: for<'r> fn(Res<'r, Master>) -> ShouldRun = Self::run_forward_log_update;
    pub (super) const RUN_CRITERIA_UPDATE_BACKWARD_LOG: for<'r> fn(Res<'r, Master>) -> ShouldRun = Self::run_backward_log_update;
    pub (super) const RUN_CRITERIA_UPDATE_LOG_END: for<'r> fn(Res<'r, Master>) -> ShouldRun = Self::run_log_end_update;

    pub (super) const RUN_CRITERIA_POST_UPDATE_FORWARD: for<'r> fn(ResMut<'r, Master>) -> ShouldRun = Self::run_forward_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_FORWARD_FAST: for<'r> fn(ResMut<'r, Master>) -> ShouldRun = Self::run_forward_fast_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_FORWARD_LOG: for<'r> fn(ResMut<'r, Master>) -> ShouldRun = Self::run_forward_log_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_BACKWARD_LOG: for<'r> fn(ResMut<'r, Master>) -> ShouldRun = Self::run_backward_log_post_update;
    pub (super) const RUN_CRITERIA_POST_UPDATE_LOG_END: for<'r> fn(ResMut<'r, Master>) -> ShouldRun = Self::run_log_end_post_update;

    pub (super) const SYSTEM_PRE_UPDATE: for<'r, 's, 't0> fn(ResMut<'r, Master>, Commands<'s, 't0>) = Self::system_pre_update;
    pub (super) const SYSTEM_POST_UPDATE: for<'r, 's, 't0, 't1> fn(ResMut<'r, Master>, Commands<'s, 't0>, ResMut<'t1, Events<ForgetEvent>>) = Self::system_post_update;

    fn run_pre_update(check: bool, mut master: ResMut<Self>, mut time: ResMut<Time>) -> ShouldRun{
        if check{
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
    fn run_update(check: bool, master: Res<Self>) -> ShouldRun{
        if check && master.pre_update_ran{
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
        Self::run_pre_update(matches!(master.progress, Progress::ForwardFast{steps: _}), master, time)
    }
    fn run_forward_log_pre_update(master: ResMut<Self>, time: ResMut<Time>) -> ShouldRun{
        Self::run_pre_update(matches!(master.progress, Progress::ForwardLog{forward_at_end: _}), master, time)
    }
    fn run_backward_log_pre_update(master: ResMut<Self>, time: ResMut<Time>) -> ShouldRun{
        Self::run_pre_update(matches!(master.progress, Progress::BackwardLog), master, time)
    }
    fn run_log_end_pre_update(mut master: ResMut<Self>) -> ShouldRun{
        if matches!(master.progress, Progress::LogEnd){
            master.pre_update_ran = true;
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_forward_update(master: Res<Self>) -> ShouldRun{
        Self::run_update(matches!(master.progress, Progress::Forward), master)
    }
    fn run_forward_fast_update(master: Res<Self>) -> ShouldRun{
        Self::run_update(matches!(master.progress, Progress::ForwardFast{steps: _}), master)
    }
    fn run_forward_log_update(master: Res<Self>) -> ShouldRun{
        Self::run_update(matches!(master.progress, Progress::ForwardLog{forward_at_end: _}), master)
    }
    fn run_backward_log_update(master: Res<Self>) -> ShouldRun{
        Self::run_update(matches!(master.progress, Progress::BackwardLog), master)
    }
    fn run_log_end_update(master: Res<Self>) -> ShouldRun{
        if matches!(master.progress, Progress::LogEnd) && master.pre_update_ran{
            ShouldRun::Yes
        } else {
            ShouldRun::No
        }
    }
    fn run_forward_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::Forward), master)
    }
    fn run_forward_fast_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::ForwardFast{steps: _}), master)
    }
    fn run_forward_log_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::ForwardLog{forward_at_end: _}), master)
    }
    fn run_backward_log_post_update(master: ResMut<Self>) -> ShouldRun{
        Self::run_post_update(matches!(master.progress, Progress::BackwardLog), master)
    }
    fn run_log_end_post_update(mut master: ResMut<Self>) -> ShouldRun{
        if matches!(master.progress, Progress::LogEnd) && master.pre_update_ran{
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
    fn forget(vec: Option<Vec<Box<dyn MasterEntryTrait>>>, commands: &mut Commands){
        vec.into_iter().flatten().for_each(|entry| entry.forget(commands));
    }
    pub fn system_pre_update(mut master: ResMut<Self>, mut commands: Commands){
        match master.progress{
            Progress::Forward | Progress::ForwardFast{steps: _} => {
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
            },
            Progress::LogEnd | Progress::Pause => {}
        }
    }
    fn post_update_forward(mut master: ResMut<Self>, mut commands: Commands, mut events: ResMut<Events<ForgetEvent>>, change_progress: bool){
        let count = events
            .drain()
            .map(|time_stamp| {
                time_stamp.0.map_or(usize::MAX, |value| (value - master.time_stamp).0 as usize)
            })
            .max();
        if let Some(mut count) = count{
            count = count.min(master.log_index); //don't forget most back entries because they contain commands for the next time_stamp
            for _ in 0..count{
                Self::forget(master.log.pop_front(), &mut commands);
            }
            master.log_index = master.log.len() - 1;
        }
        if change_progress{
            master.progress = master.progress_query;
        }
    }
    pub fn system_post_update(mut master: ResMut<Self>, mut commands: Commands, events: ResMut<Events<ForgetEvent>>)
    {
        match master.progress{
            Progress::Forward => {
                Self::post_update_forward(master, commands, events, true);
            },
            Progress::ForwardFast{mut steps} => {
                steps -= 1;
                Self::post_update_forward(master, commands, events, steps == 0);
            }
            Progress::ForwardLog{forward_at_end: _} => {
                for entry in master.log.get(master.log_index).unwrap(){
                    entry.forward(&mut commands);
                }
                if let Progress::ForwardLog { forward_at_end } = master.progress_query{
                    if master.log_index + 1 == master.log.len(){
                        if forward_at_end{
                            master.progress_query = Progress::Forward;
                        } else {
                            master.progress_query = Progress::Pause;
                        }
                    }
                }
                master.progress = master.progress_query;
            },
            Progress::BackwardLog => {
                master.time_stamp -= 1;
                master.log_index -= 1;
                if let Progress::BackwardLog = master.progress_query{
                    if master.log_index == 0{
                        master.progress_query = Progress::Pause;
                    }
                }
                master.progress = master.progress_query;
            },
            Progress::LogEnd => {
                let target = master.log_index + 1;
                while master.log.len() > target{
                    Self::forget(master.log.pop_back(), &mut commands);
                }
                master.progress = master.progress_query;
            },
            Progress::Pause => {}
        }
    }
}

