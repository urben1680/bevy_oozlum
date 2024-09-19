/*
TODO:

Features:
- reversible observer ? or issue to link
- reversible Event reader/writer
- entity commands, standard rev commands
- packed more ops
- add license
- world trait
-- run_rev_schedule
-- try_run_rev_schedule
-- run_forward_schedule
-- run_backward_schedule
-- try_run_forward_schedule
-- try_tun_backward_schedule

Enhancements:
- general todo!() und //todo
- RareStates tests
- longer log tests for RareTransition and RareState
- tests of other log methods like clear variants
- config tests
- make doctests work
- explore timestamp as u64/u32/u16 (feature?) after recent refactorings
- yeet packed int (in this crate) and replace Amount generic with const generic BYTES
-- GAT für Result<T, AmountErr>/T

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
    app::{FixedUpdate, Plugin},
    ecs::schedule::{InternedScheduleLabel, InternedSystemSet, ScheduleLabel},
    prelude::IntoSystemConfigs,
};

use meta::RevMeta;

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;

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
    pub rev_meta_sys_schedule: Option<InternedScheduleLabel>,
    pub rev_meta_sys_set: Option<InternedSystemSet>,
}

impl Default for RevSystemsPlugin {
    fn default() -> Self {
        Self {
            rev_meta: Some(RevMeta::default()),
            rev_meta_sys_schedule: Some(FixedUpdate.intern()),
            rev_meta_sys_set: None,
        }
    }
}

impl Plugin for RevSystemsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.register_type::<RevMeta>();

        if let Some(rev_meta) = &self.rev_meta {
            app.insert_resource(rev_meta.clone());
        }

        let Some(schedule) = self.rev_meta_sys_schedule else {
            return;
        };

        match self.rev_meta_sys_set {
            Some(set) => app.add_systems(schedule, RevMeta::update_world.in_set(set)),
            None => app.add_systems(schedule, RevMeta::update_world),
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
