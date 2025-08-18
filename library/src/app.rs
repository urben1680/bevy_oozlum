use bevy::{
    app::{App, FixedUpdate, Plugin},
    ecs::{
        schedule::{
            InternedScheduleLabel, InternedSystemSet, IntoScheduleConfigs, ScheduleLabel,
            Schedules, SystemSet,
        },
        system::ScheduleSystem,
    },
    utils::default,
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

// todo: rename to crate name
pub enum RevPlugin {
    Minimum,
    AddMeta(RevMeta),
    AddMetaAndRunner(RevMeta, InternedScheduleLabel),
    AddMetaAndRunnerInSet(RevMeta, InternedScheduleLabel, InternedSystemSet),
    AddRunner(InternedScheduleLabel),
    AddRunnerInSet(InternedScheduleLabel, InternedSystemSet),
}

impl Default for RevPlugin {
    fn default() -> Self {
        Self::add_meta_and_runner(default(), FixedUpdate)
    }
}

impl RevPlugin {
    pub const fn minimum() -> Self {
        Self::Minimum
    }
    pub const fn add_meta(meta: RevMeta) -> Self {
        Self::AddMeta(meta)
    }
    pub fn add_meta_and_runner(meta: RevMeta, schedule: impl ScheduleLabel) -> Self {
        Self::AddMetaAndRunner(meta, schedule.intern())
    }
    pub fn add_meta_and_runner_in_set(
        meta: RevMeta,
        schedule: impl ScheduleLabel,
        set: impl SystemSet,
    ) -> Self {
        Self::AddMetaAndRunnerInSet(meta, schedule.intern(), set.intern())
    }
    pub fn add_runner(schedule: impl ScheduleLabel) -> Self {
        Self::AddRunner(schedule.intern())
    }
    pub fn add_runner_in_set(schedule: impl ScheduleLabel, set: impl SystemSet) -> Self {
        Self::AddRunnerInSet(schedule.intern(), set.intern())
    }
}

impl Plugin for RevPlugin {
    fn build(&self, app: &mut App) {
        // add meta
        match self {
            Self::AddMeta(meta, ..)
            | Self::AddMetaAndRunner(meta, ..)
            | Self::AddMetaAndRunnerInSet(meta, ..) => {
                if let Some(existing) = app.world().get_resource::<RevMeta>() {
                    if existing != meta {
                        bevy::log::warn!(
                            "`RevSystemsPlugin::build` overwrote {existing:?} with {meta:?}"
                        );
                    }
                }
                app.insert_resource(meta.clone());
            }
            _ => {}
        };

        // add runner
        match self {
            Self::AddMetaAndRunner(_, schedule) | Self::AddRunner(schedule) => {
                app.add_systems(*schedule, RevMeta::try_run_rev_update);
            }
            Self::AddMetaAndRunnerInSet(_, schedule, set) | Self::AddRunnerInSet(schedule, set) => {
                app.add_systems(*schedule, RevMeta::try_run_rev_update.in_set(*set));
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
