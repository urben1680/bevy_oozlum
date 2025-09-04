use std::num::NonZeroU64;

use bevy::{
    app::{App, FixedUpdate, Plugin},
    ecs::{
        schedule::{
            InternedScheduleLabel, InternedSystemSet, IntoScheduleConfigs, ScheduleLabel,
            Schedules, SystemSet,
        },
        system::ScheduleSystem,
    },
};

use crate::{
    meta::RevMeta,
    schedule::{IntoRevScheduleConfigs, RevSchedule},
    undo_redo::{BundleIdOfOpCache, RevDespawnCleaner, RevDespawned, UndoRedoBuffer},
};

pub trait RevApp {
    fn rev_add_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevScheduleConfigs<ScheduleSystem, Marker>,
    ) -> &mut Self;
    fn rev_configure_sets<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        sets: impl IntoRevScheduleConfigs<InternedSystemSet, Marker>,
    ) -> &mut Self;
}

impl RevApp for App {
    fn rev_add_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevScheduleConfigs<ScheduleSystem, Marker>,
    ) -> &mut Self {
        self.world_mut()
            .resource_mut::<Schedules>()
            .entry(schedule)
            .rev_add_systems(systems);
        self
    }
    fn rev_configure_sets<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        sets: impl IntoRevScheduleConfigs<InternedSystemSet, Marker>,
    ) -> &mut Self {
        self.world_mut()
            .resource_mut::<Schedules>()
            .entry(schedule)
            .rev_configure_sets(sets);
        self
    }
}

pub enum RevPlugin {
    Minimum,
    AddMeta {
        max_world_states: Option<NonZeroU64>,
        paused: bool,
    },
    AddMetaAndRunner {
        max_world_states: Option<NonZeroU64>,
        paused: bool,
        in_schedule: InternedScheduleLabel,
    },
    AddMetaAndRunnerInSet {
        max_world_states: Option<NonZeroU64>,
        paused: bool,
        in_schedule: InternedScheduleLabel,
        in_set: InternedSystemSet,
    },
    AddRunner {
        in_schedule: InternedScheduleLabel,
    },
    AddRunnerInSet {
        in_schedule: InternedScheduleLabel,
        in_set: InternedSystemSet,
    },
}

impl Default for RevPlugin {
    fn default() -> Self {
        Self::add_meta_and_runner(
            RevMeta::DEFAULT_MAX_WORLD_STATES,
            RevMeta::DEFAULT_PAUSED,
            FixedUpdate,
        )
    }
}

impl RevPlugin {
    pub const fn minimum() -> Self {
        Self::Minimum
    }
    pub const fn add_meta(max_world_states: Option<NonZeroU64>, paused: bool) -> Self {
        Self::AddMeta {
            max_world_states,
            paused,
        }
    }
    pub fn add_meta_and_runner(
        max_world_states: Option<NonZeroU64>,
        paused: bool,
        in_schedule: impl ScheduleLabel,
    ) -> Self {
        Self::AddMetaAndRunner {
            max_world_states,
            paused,
            in_schedule: in_schedule.intern(),
        }
    }
    pub fn add_meta_and_runner_in_set(
        max_world_states: Option<NonZeroU64>,
        paused: bool,
        in_schedule: impl ScheduleLabel,
        in_set: impl SystemSet,
    ) -> Self {
        Self::AddMetaAndRunnerInSet {
            max_world_states,
            paused,
            in_schedule: in_schedule.intern(),
            in_set: in_set.intern(),
        }
    }
    pub fn add_runner(in_schedule: impl ScheduleLabel) -> Self {
        Self::AddRunner {
            in_schedule: in_schedule.intern(),
        }
    }
    pub fn add_runner_in_set(in_schedule: impl ScheduleLabel, in_set: impl SystemSet) -> Self {
        Self::AddRunnerInSet {
            in_schedule: in_schedule.intern(),
            in_set: in_set.intern(),
        }
    }
}

impl Plugin for RevPlugin {
    fn build(&self, app: &mut App) {
        // add meta
        match self {
            Self::AddMeta {
                max_world_states,
                paused,
                ..
            }
            | Self::AddMetaAndRunner {
                max_world_states,
                paused,
                ..
            }
            | Self::AddMetaAndRunnerInSet {
                max_world_states,
                paused,
                ..
            } => {
                let meta = RevMeta::new(*max_world_states, *paused);
                if let Some(existing) = app.world().get_resource::<RevMeta>() {
                    bevy::log::warn!("`RevPlugin::build` overwrote {existing:?} with {meta:?}");
                }
                app.insert_resource(meta);
            }
            _ => {}
        };

        // add runner
        match self {
            Self::AddMetaAndRunner { in_schedule, .. } | Self::AddRunner { in_schedule } => {
                app.add_systems(*in_schedule, RevMeta::try_run_rev_update);
            }
            Self::AddMetaAndRunnerInSet {
                in_schedule,
                in_set,
                ..
            }
            | Self::AddRunnerInSet {
                in_schedule,
                in_set,
            } => {
                app.add_systems(*in_schedule, RevMeta::try_run_rev_update.in_set(*in_set));
            }
            _ => {}
        }

        // other
        app.init_resource::<RevDespawnCleaner>()
            .init_resource::<UndoRedoBuffer>()
            .init_resource::<BundleIdOfOpCache>()
            .register_disabling_component::<RevDespawned>();
    }
}
