/*
TODO:

Features:
- reversible observer ? or issue to link
- reversible Event reader/writer
- entity commands, standard rev commands
- packed more ops
- add license

Enhancements:
- general todo!() und //todo, reduce unwrap/expect
- RareStates tests
- longer log tests for RareTransition and RareState
- tests of other log methods like clear variants
- config tests
- forward_log_by_timestamp / backward_log_by_timestamp for all logs and testing
- meta-free methods of logs, meta offers fitting methods

Docs
- examples
- documentations, besonders mit informationen welche Methoden für deterministische Logik geeignet ist


UNSUPPORTED:

- Change Detection
- Hooks
- IntoSystemConfigs::distributive_run_if
- Schedule::set_apply_final_deferred
- Schedule::graph_mut
- ScheduleBuildSettings::auto_insert_apply_deferred
*/

use std::hash::Hash;

use bevy::{
    app::Plugin,
    ecs::schedule::{InternedScheduleLabel, ScheduleLabel},
};

use meta::RevMeta;

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSchedule(InternedScheduleLabel);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(InternedScheduleLabel);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

pub struct RevSystemsPlugin;

impl Plugin for RevSystemsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.register_type::<RevMeta>();
    }
}
