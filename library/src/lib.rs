/*
TODO:

Features:
- reversible observer ? or issue to link
- reversible Event reader/writer
- entity commands, standard rev commands
- add license
- world trait
-- run_rev_schedule
-- try_run_rev_schedule
-- run_forward_schedule
-- run_backward_schedule
-- try_run_forward_schedule
-- try_tun_backward_schedule
-- rev_schedule_scope
-- try_rev_schedule_scope

Enhancements:
- reduce todo!() and //todo
- RareStates tests
- longer log tests for RareTransition and RareState
- tests of other log methods like clear variants
- config tests
- make doctests work

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
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        query::Access,
        schedule::{InternedScheduleLabel, InternedSystemSet, ScheduleLabel},
    },
    prelude::{IntoSystemConfigs, SystemSet},
};

use meta::RevMeta;

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod set_configs;
pub mod system_configs;

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

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardCmdsSys(InternedSystemSet);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSys(InternedSystemSet);

static EMPTY_COMPONENT_ACCESS: Access<ComponentId> = Access::new();
static EMPTY_ARCHETYPE_COMPONENT_ACCESS: Access<ArchetypeComponentId> = Access::new();

fn check_tick(own_tick: &mut Tick, change_tick: Tick) {
    // reference: Tick::check_tick
    let age = change_tick.get().wrapping_sub(own_tick.get());
    if age > Tick::MAX.get() {
        *own_tick = Tick::new(change_tick.get().wrapping_sub(Tick::MAX.get()));
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
