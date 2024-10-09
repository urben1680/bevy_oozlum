use bevy::{
    app::App,
    ecs::schedule::ScheduleLabel,
    prelude::{Schedule, Schedules},
};

pub use crate::{
    schedule::RevSchedule, set_configs::IntoRevSystemSetConfigs,
    system_configs::IntoRevSystemConfigs,
};

use crate::{meta::CommandsLogReducings, BackwardSchedule, ForwardSchedule};

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
    fn rev_init_schedule(&mut self, label: impl ScheduleLabel) -> &mut Self;
    fn rev_add_schedule(&mut self, schedule: RevSchedule) -> &mut Self;
    fn rev_edit_schedule(
        &mut self,
        label: impl ScheduleLabel,
        f: impl FnMut(&mut RevSchedule),
    ) -> &mut Self;
}

impl RevApp for App {
    fn rev_add_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevSystemConfigs<Marker>,
    ) -> &mut Self {
        let schedule = schedule.intern();
        let mut configs = systems.into_rev_configs();
        self.world_mut()
            .get_resource_or_insert_with(CommandsLogReducings::default)
            .0
            .append(&mut configs.commands_logged_at_reductions);
        self.add_systems(ForwardSchedule(schedule), configs.forward)
            .add_systems(BackwardSchedule(schedule), configs.backward)
            .rev_configure_sets(schedule, configs.set_configs)
    }
    fn rev_configure_sets<Marker>(
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
    fn rev_init_schedule(&mut self, label: impl ScheduleLabel) -> &mut Self {
        let label = label.intern();
        self.init_schedule(ForwardSchedule(label))
            .init_schedule(BackwardSchedule(label))
    }
    fn rev_add_schedule(&mut self, schedule: RevSchedule) -> &mut Self {
        self.add_schedule(schedule.forward)
            .add_schedule(schedule.backward)
    }
    fn rev_edit_schedule(
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
        let mut rev_schedule = RevSchedule {
            forward,
            backward,
            commands_logged_at_reductions: Vec::new(),
        };
        f(&mut rev_schedule);
        schedules.insert(rev_schedule.forward);
        schedules.insert(rev_schedule.backward);
        self.world_mut()
            .get_resource_or_insert_with(CommandsLogReducings::default)
            .0
            .append(&mut rev_schedule.commands_logged_at_reductions);
        self
    }
}
