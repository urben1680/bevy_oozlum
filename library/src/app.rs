use bevy::{
    app::App,
    ecs::schedule::{ScheduleLabel, Schedules},
};

pub use crate::{
    schedule::RevSchedule, set_configs::IntoRevSystemSetConfigs,
    system_configs::IntoRevSystemConfigs,
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
