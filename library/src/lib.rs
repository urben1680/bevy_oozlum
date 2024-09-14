/*
TODO:

Features:
- reversible observer ? or issue to link
- reversible Event reader/writer
- entity commands, standard rev commands
- tuple impl configs
- packed more ops

Enhancements:
- general todo!() und //todo, reduce unwrap/expect
- longer log tests for RareTransition and RareValue
- tests of other log methods like clear variants

Docs
- examples
- documentations, besonders mit informationen welche Methoden für deterministische Logik geeignet ist


UNSUPPORTED:

- Change Detection
- Hooks
- IntoSystemConfigs::rev_distributive_run_if
- Schedule::set_apply_final_deferred
- Schedule::graph_mut
- ScheduleBuildSettings::auto_insert_apply_deferred
*/

use std::hash::Hash;

use bevy::{
    app::Plugin,
    ecs::{intern::Interned, schedule::ScheduleLabel},
};
use meta::RevMeta;

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

pub struct RevSystemsPlugin;

impl Plugin for RevSystemsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.register_type::<RevMeta>();
    }
}
