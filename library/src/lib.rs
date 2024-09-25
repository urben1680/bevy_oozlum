/*
TODO:

Features:
- reversible Event reader/writer
- entity commands, standard rev commands
-- postponed to bevy 0.15 due to required components + disabled entities
- add license

Enhancements:
- reduce todo!() and //todo
- RareStates tests
- longer log tests for RareTransition and RareState
- tests of other log methods like clear variants
- config tests
- make doctests work
- mod/use/pub cleanup, make prelude
- setting for state logs that serde only takes the current state into account (default?)
- setting for logs to serde the current capacity as well
-- all as functions for #[serde(with = "module")], uses $module::serialize and $module::deserialize
                            State   Transition
default                     full    full
state_only                  x       -
with_capacity               x       x
state_only_with_capacity    x       -

- observer tests
- hook -> observer -> commands reversible order test
-- commands should also trigger hooks here
-- commands shouls also call observer here
-- a second sync point aftwerwards should be scheduled to assert nothing ist postponed into it

Docs
- examples
- documentations, besonders mit informationen welche Methoden für deterministische Logik geeignet ist

UNSUPPORTED:

- Change Detection
- IntoSystemConfigs::distributive_run_if
- Schedule::set_apply_final_deferred
- Schedule::graph_mut
- ScheduleBuildSettings::auto_insert_apply_deferred
- Trigger::event_mut
*/

use std::hash::Hash;

use bevy::{
    app::{FixedUpdate, Plugin},
    ecs::schedule::{InternedScheduleLabel, InternedSystemSet, IntoSystemConfigs, ScheduleLabel},
};

use commands::RevCommandBuffer;
use meta::RevMeta;

pub mod app;
pub mod commands;
pub mod event;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod set_configs;
pub mod system_configs;
pub mod world;

/// Should not be pub to not add invalid settings (see unsupported Schedule settings)
#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSchedule(InternedScheduleLabel);

/// Should not be pub to not add invalid settings (see unsupported Schedule settings)
#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(InternedScheduleLabel);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

pub struct RevSystemsPlugin {
    pub rev_meta: Option<RevMeta>,
    pub add_rev_meta_sys_in: Option<(InternedScheduleLabel, Option<InternedSystemSet>)>,
}

impl Default for RevSystemsPlugin {
    fn default() -> Self {
        Self {
            rev_meta: Some(RevMeta::default()),
            add_rev_meta_sys_in: Some((FixedUpdate.intern(), None)),
        }
    }
}

impl Plugin for RevSystemsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.register_type::<RevMeta>()
            // needs to be manually inserted because first accessor might be a hook which cannot insert it
            .init_resource::<RevCommandBuffer>();

        if let Some(rev_meta) = &self.rev_meta {
            app.insert_resource(rev_meta.clone());
        }

        match self.add_rev_meta_sys_in {
            Some((schedule, None)) => app.add_systems(schedule, RevMeta::update_world),
            Some((schedule, Some(set))) => {
                app.add_systems(schedule, RevMeta::update_world.in_set(set))
            }
            None => app,
        };
    }
}

macro_rules! error_per_flag {
    ($flag:expr, $($arg:tt)+) => ({
        if !*$flag {
            bevy::utils::tracing::error!($($arg)+);
            *$flag = true;
        }
        core::default::Default::default()
    });
}

pub(crate) use error_per_flag;
