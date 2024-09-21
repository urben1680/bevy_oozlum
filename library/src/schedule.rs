use bevy::ecs::{
    schedule::{
        ExecutorKind, LogLevel, Schedule, ScheduleBuildError, ScheduleBuildSettings, ScheduleGraph,
        ScheduleLabel,
    },
    world::World,
};

use crate::{
    meta::{Direction, RevMeta},
    BackwardSchedule, ForwardSchedule,
};

use super::{set_configs::IntoRevSystemSetConfigs, system_configs::IntoRevSystemConfigs};

pub struct RevSchedule {
    pub(crate) forward: Schedule,
    pub(crate) backward: Schedule,
}

pub struct RevScheduleBuildSettings {
    pub ambiguity_detection: LogLevel,
    pub hierarchy_detection: LogLevel,
    pub use_shortnames: bool,
    pub report_sets: bool,
}

pub enum TryRunError {
    RevMetaMissing,
    RevMetaWrongDirection(RevMeta),
}

impl RevSchedule {
    pub fn new(label: impl ScheduleLabel) -> Self {
        let label = label.intern();
        Self {
            forward: Schedule::new(ForwardSchedule(label)),
            backward: Schedule::new(BackwardSchedule(label)),
        }
    }
    pub fn add_rev_systems<Marker>(
        &mut self,
        systems: impl IntoRevSystemConfigs<Marker>,
    ) -> &mut Self {
        let configs = systems.into_rev_configs();
        self.forward.add_systems(configs.forward);
        self.backward.add_systems(configs.backward);
        self.configure_rev_sets(configs.set_configs);
        self
    }
    pub fn configure_rev_sets<Marker>(
        &mut self,
        sets: impl IntoRevSystemSetConfigs<Marker>,
    ) -> &mut Self {
        let configs = sets.into_rev_configs();
        self.forward.configure_sets(configs.forward_sys);
        self.backward
            .configure_sets((configs.backward_cmds_sys, configs.backward_sys));
        self
    }
    pub fn set_build_setting(&mut self, settings: RevScheduleBuildSettings) -> &mut Self {
        let RevScheduleBuildSettings {
            ambiguity_detection,
            hierarchy_detection,
            use_shortnames,
            report_sets,
        } = settings;
        let settings = ScheduleBuildSettings {
            ambiguity_detection,
            hierarchy_detection,
            auto_insert_apply_deferred: true,
            use_shortnames,
            report_sets,
        };
        self.forward.set_build_settings(settings.clone());
        self.backward.set_build_settings(settings);
        self
    }
    pub fn get_build_settings(&self) -> RevScheduleBuildSettings {
        let ScheduleBuildSettings {
            ambiguity_detection,
            hierarchy_detection,
            use_shortnames,
            report_sets,
            ..
        } = self.forward.get_build_settings();
        RevScheduleBuildSettings {
            ambiguity_detection,
            hierarchy_detection,
            use_shortnames,
            report_sets,
        }
    }
    pub fn get_executor_kind(&self) -> ExecutorKind {
        self.forward.get_executor_kind()
    }
    pub fn set_executor_kind(&mut self, executor: ExecutorKind) -> &mut Self {
        self.forward.set_executor_kind(executor);
        self.backward.set_executor_kind(executor);
        self
    }
    pub fn try_run(&mut self, world: &mut World) -> Result<(), TryRunError> {
        let meta = world
            .get_resource::<RevMeta>()
            .ok_or(TryRunError::RevMetaMissing)?
            .clone();
        match meta.get_direction() {
            Some(Direction::Forward) | Some(Direction::ForwardLog) => Ok(self.forward.run(world)),
            Some(Direction::BackwardLog) => Ok(self.backward.run(world)),
            None => Err(TryRunError::RevMetaWrongDirection(meta)),
        }
    }
    pub fn run_forward(&mut self, world: &mut World) {
        self.forward.run(world)
    }
    pub fn run_backward(&mut self, world: &mut World) {
        self.backward.run(world)
    }
    pub fn initialize(
        &mut self,
        world: &mut World,
    ) -> (
        Result<(), ScheduleBuildError>,
        Result<(), ScheduleBuildError>,
    ) {
        (
            self.forward.initialize(world),
            self.backward.initialize(world),
        )
    }
    pub fn graphs(&self) -> (&ScheduleGraph, &ScheduleGraph) {
        (self.forward.graph(), self.backward.graph())
    }
    pub fn systems_len(&self) -> usize {
        self.forward.systems_len()
    }
}
