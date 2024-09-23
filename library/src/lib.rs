/*
TODO:

Features:
- hooks
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
- setting for state logs that serde only takes the current state into account
- observer tests
- commands -> observer -> hook reversible order test

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

impl ForwardSchedule {
    fn of(label: impl ScheduleLabel) -> Self {
        Self(label.intern())
    }
}

/// Should not be pub to not add invalid settings (see unsupported Schedule settings)
#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(InternedScheduleLabel);

impl BackwardSchedule {
    fn of(label: impl ScheduleLabel) -> Self {
        Self(label.intern())
    }
}

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
        app.register_type::<RevMeta>();

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
