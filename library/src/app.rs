use core::num::NonZeroU64;

use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::{
    schedule::{
        InternedScheduleLabel, InternedSystemSet, IntoScheduleConfigs, ScheduleLabel, Schedules,
        SystemSet,
    },
    system::ScheduleSystem,
};

use crate::{
    meta::RevMeta,
    schedule::{IntoRevScheduleConfigs, RevSchedule},
    undo_redo::RevDespawned,
};

/// Extension trait for [`App`] with reversible variants of various methods.
pub trait RevApp {
    /// Reversible version of [`App::add_systems`].
    ///
    /// Does not support exclusive systems.
    fn rev_add_systems<Marker>(
        &mut self,
        schedule: impl ScheduleLabel,
        systems: impl IntoRevScheduleConfigs<ScheduleSystem, Marker>,
    ) -> &mut Self;

    /// Reversible version of [`App::configure_sets`].
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
        max_past_len: NonZeroU64,
        paused: bool,
    },
    AddMetaAndRunner {
        max_past_len: NonZeroU64,
        paused: bool,
        in_schedule: InternedScheduleLabel,
    },
    AddMetaAndRunnerInSet {
        max_past_len: NonZeroU64,
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
            RevMeta::DEFAULT_MAX_PAST_LEN,
            RevMeta::DEFAULT_PAUSED,
            FixedUpdate,
        )
    }
}

impl RevPlugin {
    pub const fn minimum() -> Self {
        Self::Minimum
    }
    pub const fn add_meta(max_past_len: NonZeroU64, paused: bool) -> Self {
        Self::AddMeta {
            max_past_len,
            paused,
        }
    }
    pub fn add_meta_and_runner(
        max_past_len: NonZeroU64,
        paused: bool,
        in_schedule: impl ScheduleLabel,
    ) -> Self {
        Self::AddMetaAndRunner {
            max_past_len,
            paused,
            in_schedule: in_schedule.intern(),
        }
    }
    pub fn add_meta_and_runner_in_set(
        max_past_len: NonZeroU64,
        paused: bool,
        in_schedule: impl ScheduleLabel,
        in_set: impl SystemSet,
    ) -> Self {
        Self::AddMetaAndRunnerInSet {
            max_past_len,
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
                max_past_len,
                paused,
                ..
            }
            | Self::AddMetaAndRunner {
                max_past_len,
                paused,
                ..
            }
            | Self::AddMetaAndRunnerInSet {
                max_past_len,
                paused,
                ..
            } => {
                let meta = RevMeta::new(*max_past_len, *paused);
                if let Some(existing) = app.world().get_resource::<RevMeta>() {
                    bevy_log::warn!("`RevPlugin::build` overwrote {existing:?} with {meta:?}");
                }
                app.insert_resource(meta);
            }
            _ => {}
        };

        // add runner
        match self {
            Self::AddMetaAndRunner { in_schedule, .. } | Self::AddRunner { in_schedule } => {
                app.add_systems(*in_schedule, RevMeta::run_rev_update);
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
                app.add_systems(*in_schedule, RevMeta::run_rev_update.in_set(*in_set));
            }
            _ => {}
        }

        // other
        app.register_disabling_component::<RevDespawned>();
    }
}
