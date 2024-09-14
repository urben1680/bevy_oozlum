/*
TODO:

Features:
- reversible observer ? or issue to link
- reversible Event reader/writer
- entity commands, standard rev commands
- App Methoden auch für RevSchedule struct
-- apply_final_deferred Option genauer betrachten
- tuple impl configs

Enhancements:
- general todo!() und //todo, reduce unwrap/expect
- derive Reflection + serde
-- done for logs + inner structs
- plugin registers types
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

use bevy::ecs::{intern::Interned, schedule::ScheduleLabel};

pub mod app; // todo: rename, put app into a sub module and cfg gate it
pub mod commands;
pub mod log;
pub mod meta;

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(Interned<dyn ScheduleLabel>);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;
