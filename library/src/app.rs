use bevy::{
    app::App,
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        intern::Interned,
        query::Access,
        schedule::{ScheduleLabel, SystemSet},
    },
};
use set_configs::IntoRevSystemSetConfigs;
use system_configs::IntoRevSystemConfigs;

use crate::{BackwardSchedule, ForwardSchedule};

mod set_configs;
mod system_configs;

pub trait RevApp {
    fn add_rev_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevSystemConfigs<Marker>,
    ) -> &mut Self;
    fn configure_rev_sets(
        &mut self,
        schedule: impl ScheduleLabel,
        sets: impl IntoRevSystemSetConfigs,
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
    fn configure_rev_sets(
        &mut self,
        schedule: impl ScheduleLabel,
        sets: impl IntoRevSystemSetConfigs,
    ) -> &mut Self {
        let schedule = schedule.intern();
        let configs = sets.into_rev_configs();
        self.configure_sets(ForwardSchedule(schedule), configs.forward_sys)
            .configure_sets(BackwardSchedule(schedule), configs.backward_cmds_sys)
            .configure_sets(BackwardSchedule(schedule), configs.backward_sys)
    }
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardCmdsSys(Interned<dyn SystemSet>);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSys(Interned<dyn SystemSet>);

static EMPTY_COMPONENT_ACCESS: Access<ComponentId> = Access::new();
static EMPTY_ARCHETYPE_COMPONENT_ACCESS: Access<ArchetypeComponentId> = Access::new();

fn check_tick(own_tick: &mut Tick, change_tick: Tick) {
    // reference: Tick::check_tick
    let age = change_tick.get().wrapping_sub(own_tick.get());
    if age > Tick::MAX.get() {
        *own_tick = Tick::new(change_tick.get().wrapping_sub(Tick::MAX.get()));
    }
}
