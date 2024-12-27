use bevy::{
    app::{App, FixedUpdate, Plugin},
    ecs::schedule::{
        InternedScheduleLabel, InternedSystemSet, IntoSystemConfigs, ScheduleLabel, Schedules,
        SystemSet,
    },
    utils::default,
};

use crate::{
    meta::RevMeta,
    schedule::{IntoRevSystemConfigs, IntoRevSystemSetConfigs, RevSchedule},
};

pub trait RevApp {
    fn rev_add_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevSystemConfigs<Marker>,
    ) -> &mut Self;
    fn rev_configure_sets<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self;
}

impl RevApp for App {
    fn rev_add_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevSystemConfigs<Marker>,
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
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self {
        self.world_mut()
            .resource_mut::<Schedules>()
            .entry(schedule)
            .rev_configure_sets(sets);
        self
    }
}

pub enum RevSystemsPlugin {
    AddMeta(RevMeta),
    AddMetaAndRunner(RevMeta, InternedScheduleLabel),
    AddMetaAndRunnerInSet(RevMeta, InternedScheduleLabel, InternedSystemSet),
    AddRunner(InternedScheduleLabel),
    AddRunnerInSet(InternedScheduleLabel, InternedSystemSet),
}

impl Default for RevSystemsPlugin {
    fn default() -> Self {
        Self::add_meta_and_runner(default(), FixedUpdate)
    }
}

impl RevSystemsPlugin {
    pub fn add_meta(meta: RevMeta) -> Self {
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

impl Plugin for RevSystemsPlugin {
    fn build(&self, app: &mut App) {
        match self {
            Self::AddMeta(meta, ..)
            | Self::AddMetaAndRunner(meta, ..)
            | Self::AddMetaAndRunnerInSet(meta, ..) => {
                app.insert_resource(meta.clone());
            }
            _ => {}
        };
        match self {
            Self::AddMetaAndRunner(_, schedule) | Self::AddRunner(schedule) => {
                app.add_systems(*schedule, RevMeta::update_world);
            }
            Self::AddMetaAndRunnerInSet(_, schedule, set) | Self::AddRunnerInSet(schedule, set) => {
                app.add_systems(*schedule, RevMeta::update_world.in_set(*set));
            }
            Self::AddMeta(..) => {}
        }
    }
}
