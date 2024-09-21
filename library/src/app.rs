use bevy::{
    app::App,
    ecs::schedule::ScheduleLabel,
    prelude::{Schedule, Schedules},
};

pub use crate::{
    schedule::RevSchedule, set_configs::IntoRevSystemSetConfigs,
    system_configs::IntoRevSystemConfigs,
};

use crate::{BackwardSchedule, ForwardSchedule};

pub trait RevApp {
    fn add_rev_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevSystemConfigs<Marker>,
    ) -> &mut Self;
    fn configure_rev_sets<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self;
    fn init_rev_schedule(&mut self, label: impl ScheduleLabel) -> &mut Self;
    fn add_rev_schedule(&mut self, schedule: RevSchedule) -> &mut Self;
    fn edit_rev_schedule(
        &mut self,
        label: impl ScheduleLabel,
        f: impl FnMut(&mut RevSchedule),
    ) -> &mut Self;
}

impl RevApp for App {
    fn add_rev_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevSystemConfigs<Marker>,
    ) -> &mut Self {
        let schedule = schedule.intern();
        let configs = systems.into_rev_configs();
        self.add_systems(ForwardSchedule(schedule), configs.forward)
            .add_systems(BackwardSchedule(schedule), configs.backward)
            .configure_rev_sets(schedule, configs.set_configs)
    }
    fn configure_rev_sets<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self {
        let schedule = schedule.intern();
        let configs = sets.into_rev_configs();
        self.configure_sets(ForwardSchedule(schedule), configs.forward_sys)
            .configure_sets(
                BackwardSchedule(schedule),
                (configs.backward_cmds_sys, configs.backward_sys),
            )
    }
    fn init_rev_schedule(&mut self, label: impl ScheduleLabel) -> &mut Self {
        let label = label.intern();
        self.init_schedule(ForwardSchedule(label))
            .init_schedule(BackwardSchedule(label))
    }
    fn add_rev_schedule(&mut self, schedule: RevSchedule) -> &mut Self {
        self.add_schedule(schedule.forward)
            .add_schedule(schedule.backward)
    }
    fn edit_rev_schedule(
        &mut self,
        label: impl ScheduleLabel,
        mut f: impl FnMut(&mut RevSchedule),
    ) -> &mut Self {
        let label = label.intern();
        let forward_label = ForwardSchedule(label);
        let backward_label = BackwardSchedule(label);
        let mut schedules = self.world_mut().resource_mut::<Schedules>();
        let forward = schedules
            .remove(forward_label)
            .unwrap_or_else(|| Schedule::new(forward_label));
        let backward = schedules
            .remove(backward_label)
            .unwrap_or_else(|| Schedule::new(backward_label));
        let mut rev_schedule = RevSchedule { forward, backward };
        f(&mut rev_schedule);
        schedules.insert(rev_schedule.forward);
        schedules.insert(rev_schedule.backward);
        self
    }
}
