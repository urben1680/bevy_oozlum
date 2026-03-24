//! This module contains [`RevPlugin`] and an extension trait to add reversible systems and
//! configure system sets at the app level.

use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::{
    schedule::{
        InternedScheduleLabel, InternedSystemSet, IntoScheduleConfigs, ScheduleLabel, Schedules,
        SystemSet,
    },
    system::ScheduleSystem,
};
use bevy_log::warn;

use crate::{
    meta::{RevMeta, run_rev_update},
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

/// Plugin for the [`bevy_oozlum`](crate) crate.
///
/// Always registers [`RevDespawned`] as a disabling component.
///
/// If left unchanged, this plugin
/// - inserts the [`RevMeta`] resource, unpaused with a maximum past len of `1` frame
/// - adds [`run_rev_update`] to [`FixedUpdate`]
pub struct RevPlugin;

impl RevPlugin {
    /// Unsets [`RevMeta`] insertion. With this the insertion needs to be done manually.
    pub fn unset_meta(self) -> ModifiedRevPlugin {
        ModifiedRevPlugin::default().unset_meta()
    }

    /// Sets the maximum amount of frames that can be reversed by.
    ///
    /// The value is stored in [`RevMeta`]. Using [`unset_meta`] will ignore prior calls of this.
    ///
    /// [`unset_meta`]: Self::unset_meta
    pub fn set_max_past_len(self, max_past_len: u64) -> ModifiedRevPlugin {
        ModifiedRevPlugin::default().set_max_past_len(max_past_len)
    }

    /// Sets [`RevMeta`] to paused. With this unpausing needs to be done manually. Using
    /// [`unset_meta`] will ignore prior calls of this.
    ///
    /// [`unset_meta`]: Self::unset_meta
    pub fn set_paused(self) -> ModifiedRevPlugin {
        ModifiedRevPlugin::default().set_paused()
    }

    /// Unsets [`run_rev_update`] addition to an schedule.
    pub fn unset_runner(self) -> ModifiedRevPlugin {
        ModifiedRevPlugin::default().unset_runner()
    }

    /// Sets in which schedule [`run_rev_update`] will be added to. Using [`unset_runner`]
    /// will ignore prior calls of this.
    ///
    /// [`unset_runner`]: Self::unset_runner
    pub fn set_runner_in_schedule(self, schedule: impl ScheduleLabel) -> ModifiedRevPlugin {
        ModifiedRevPlugin::default().set_runner_in_schedule(schedule)
    }

    /// Sets in which system set [`run_rev_update`] will be added to. Using
    /// [`unset_runner`] will ignore prior calls of this.
    ///
    /// [`unset_runner`]: Self::unset_runner
    pub fn set_runner_in_set(self, set: impl SystemSet) -> ModifiedRevPlugin {
        ModifiedRevPlugin::default().set_runner_in_set(set)
    }
}

/// Plugin for the [`bevy_oozlum`](crate) crate.
///
/// Always registers [`RevDespawned`] as a disabling component.
pub struct ModifiedRevPlugin {
    meta: Option<(u64, bool)>,
    runner: Option<(InternedScheduleLabel, Option<InternedSystemSet>)>,
}

impl ModifiedRevPlugin {
    const META_DEFAULT: (u64, bool) = (RevMeta::DEFAULT_MAX_PAST_LEN, RevMeta::DEFAULT_PAUSED);

    /// Unsets [`RevMeta`] insertion. With this the insertion needs to be done manually.
    pub fn unset_meta(mut self) -> ModifiedRevPlugin {
        if self.meta.is_some_and(|meta| meta != Self::META_DEFAULT) {
            warn!("overwrote plugin change with RevUpdate::unset_meta");
        }
        self.meta = None;
        self
    }

    /// Sets the maximum amount of frames that can be reversed by.
    ///
    /// The value is stored in [`RevMeta`]. Using [`unset_meta`] will ignore prior calls of this.
    ///
    /// [`unset_meta`]: Self::unset_meta
    pub fn set_max_past_len(mut self, max_past_len: u64) -> ModifiedRevPlugin {
        match self.meta.as_mut() {
            Some((max_past_len_mut, _)) => {
                if *max_past_len_mut != RevMeta::DEFAULT_MAX_PAST_LEN {
                    warn!("overwrote plugin change with RevUpdate::set_max_past_len");
                }
                *max_past_len_mut = max_past_len;
            }
            None => {
                warn!("overwrote plugin change with RevUpdate::set_max_past_len");
                self.meta = Some((max_past_len, RevMeta::DEFAULT_PAUSED));
            }
        }
        self
    }

    /// Sets [`RevMeta`] to paused. With this unpausing needs to be done manually. Using
    /// [`unset_meta`] will ignore prior calls of this.
    ///
    /// [`unset_meta`]: Self::unset_meta
    pub fn set_paused(mut self) -> ModifiedRevPlugin {
        match self.meta.as_mut() {
            Some((_, paused_mut)) => {
                *paused_mut = false;
            }
            None => {
                warn!("overwrote plugin change with RevUpdate::set_paused");
                self.meta = Some((RevMeta::DEFAULT_MAX_PAST_LEN, false));
            }
        }
        self
    }

    /// Unsets [`run_rev_update`] addition to an schedule. Using [`set_runner_in_schedule`]
    /// pr [`set_runner_in_set`] will ignore prior calls of this.
    ///
    /// [`set_runner_in_schedule`]: Self::set_runner_in_schedule
    /// [`set_runner_in_set`]: Self::set_runner_in_set
    pub fn unset_runner(mut self) -> ModifiedRevPlugin {
        if self
            .runner
            .is_some_and(|(schedule, set)| schedule != FixedUpdate.intern() || set.is_some())
        {
            warn!("overwrote plugin change with RevUpdate::unset_runner");
        }
        self.runner = None;
        self
    }

    /// Sets in which schedule [`run_rev_update`] will be added to. Using [`unset_runner`]
    /// will ignore prior calls of this.
    ///
    /// [`unset_runner`]: Self::unset_runner
    pub fn set_runner_in_schedule(mut self, schedule: impl ScheduleLabel) -> ModifiedRevPlugin {
        match self.runner.as_mut() {
            Some((schedule_mut, _)) => {
                if *schedule_mut != FixedUpdate.intern() {
                    warn!("overwrote plugin change with RevUpdate::set_runner_in_schedule");
                }
                *schedule_mut = schedule.intern();
            }
            None => {
                warn!("overwrote plugin change with RevUpdate::set_runner_in_schedule");
                self.runner = Some((schedule.intern(), None));
            }
        }
        self
    }

    /// Sets in which system set [`run_rev_update`] will be added to. Using
    /// [`unset_runner`] will ignore prior calls of this.
    ///
    /// [`unset_runner`]: Self::unset_runner
    pub fn set_runner_in_set(mut self, set: impl SystemSet) -> ModifiedRevPlugin {
        match self.runner.as_mut() {
            Some((_, Some(set_mut))) => {
                warn!("overwrote plugin change with RevUpdate::set_runner_in_set");
                *set_mut = set.intern();
            }
            Some((_, set_mut)) => {
                *set_mut = Some(set.intern());
            }
            None => {
                warn!("overwrote plugin change with RevUpdate::set_runner_in_set");
                self.runner = Some((FixedUpdate.intern(), Some(set.intern())));
            }
        }
        self
    }
}

impl Default for ModifiedRevPlugin {
    fn default() -> Self {
        Self {
            meta: Some(Self::META_DEFAULT),
            runner: Some((FixedUpdate.intern(), None)),
        }
    }
}

impl Plugin for RevPlugin {
    fn build(&self, app: &mut App) {
        app.register_disabling_component::<RevDespawned>();
        app.init_resource::<RevMeta>();
        app.add_systems(FixedUpdate, run_rev_update);
    }
}

impl Plugin for ModifiedRevPlugin {
    fn build(&self, app: &mut App) {
        app.register_disabling_component::<RevDespawned>();
        if let Some((max_past_len, paused)) = self.meta {
            app.insert_resource(RevMeta::new(max_past_len, paused));
        }
        match self.runner {
            Some((schedule, None)) => {
                app.add_systems(schedule, run_rev_update);
            }
            Some((schedule, Some(set))) => {
                app.add_systems(schedule, run_rev_update.in_set(set));
            }
            None => {}
        }
    }
}
