use bevy::{
    app::{App, FixedUpdate, Plugin}, ecs::schedule::{InternedScheduleLabel, InternedSystemSet, ScheduleLabel, Schedules, IntoSystemConfigs}, utils::default
};

use crate::{commands::RevCommandBuffer, meta::RevMeta};
pub use crate::schedule::{IntoRevSystemConfigs, IntoRevSystemSetConfigs, RevSchedule};

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

pub struct RevSystemsPlugin {
    pub rev_meta: Option<RevMeta>,
    pub add_rev_meta_sys_in: Option<(InternedScheduleLabel, Option<InternedSystemSet>)>,
}

impl Default for RevSystemsPlugin {
    fn default() -> Self {
        Self {
            rev_meta: Some(default()),
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

        let Some((schedule, set)) = self.add_rev_meta_sys_in else {
            return;
        };

        match set {
            Some(set) => app.add_systems(schedule, RevMeta::update_world.in_set(set)),
            None => app.add_systems(schedule, RevMeta::update_world),
        };
    }
}
