use bevy::ecs::{
    schedule::{ScheduleLabel, Schedules},
    world::World,
};

use crate::{
    app::RevSchedule,
    meta::{Direction, RevMeta, RevTryRunScheduleError},
    BackwardSchedule, ForwardSchedule, RevUpdate,
};

#[derive(Clone, Copy, Debug)]
pub struct ScheduleMissing;

pub trait RevWorld {
    fn rev_run_schedule(&mut self, label: impl ScheduleLabel);
    fn rev_try_run_schedule(
        &mut self,
        label: impl ScheduleLabel,
    ) -> Result<(), RevTryRunScheduleError>;
    fn rev_run_forward_schedule(&mut self, label: impl ScheduleLabel);
    fn rev_try_run_forward_schedule(
        &mut self,
        label: impl ScheduleLabel,
    ) -> Result<(), ScheduleMissing>;
    fn rev_run_backward_schedule(&mut self, label: impl ScheduleLabel);
    fn rev_try_run_backward_schedule(
        &mut self,
        label: impl ScheduleLabel,
    ) -> Result<(), ScheduleMissing>;
    fn rev_schedule_scope<R>(
        &mut self,
        label: impl ScheduleLabel,
        f: impl FnOnce(&mut World, &mut RevSchedule) -> R,
    ) -> R;
    fn rev_try_schedule_scope<R>(
        &mut self,
        label: impl ScheduleLabel,
        f: impl FnOnce(&mut World, &mut RevSchedule) -> R,
    ) -> Result<R, ScheduleMissing>;
}

impl RevWorld for World {
    fn rev_run_schedule(&mut self, label: impl ScheduleLabel) {
        self.rev_try_run_schedule(label).unwrap()
    }
    fn rev_try_run_schedule(
        &mut self,
        label: impl ScheduleLabel,
    ) -> Result<(), RevTryRunScheduleError> {
        if label.intern() == RevUpdate.intern() {
            RevMeta::try_update_world(self)
        } else {
            let meta = self
                .get_resource::<RevMeta>()
                .ok_or(RevTryRunScheduleError::RevMetaMissing)?
                .clone();
            let direction = meta
                .get_direction()
                .ok_or_else(|| RevTryRunScheduleError::NoRevScheduleRunning(meta.clone()))?;
            let label = label.intern();
            match direction {
                Direction::Forward { .. } => self.rev_try_run_forward_schedule(label),
                Direction::BackwardLog => self.rev_try_run_backward_schedule(label),
            }
            .map_err(|_| RevTryRunScheduleError::ScheduleMissing {
                meta,
                schedule: format!("{label:?}"),
            })
        }
    }
    fn rev_run_forward_schedule(&mut self, label: impl ScheduleLabel) {
        self.rev_try_run_forward_schedule(label).unwrap()
    }
    fn rev_try_run_forward_schedule(
        &mut self,
        label: impl ScheduleLabel,
    ) -> Result<(), ScheduleMissing> {
        self.try_run_schedule(ForwardSchedule(label.intern()))
            .map_err(|_| ScheduleMissing)
    }
    fn rev_run_backward_schedule(&mut self, label: impl ScheduleLabel) {
        self.rev_try_run_backward_schedule(label).unwrap()
    }
    fn rev_try_run_backward_schedule(
        &mut self,
        label: impl ScheduleLabel,
    ) -> Result<(), ScheduleMissing> {
        self.try_run_schedule(BackwardSchedule(label.intern()))
            .map_err(|_| ScheduleMissing)
    }
    fn rev_schedule_scope<R>(
        &mut self,
        label: impl ScheduleLabel,
        f: impl FnOnce(&mut World, &mut RevSchedule) -> R,
    ) -> R {
        self.rev_try_schedule_scope(label, f).unwrap()
    }
    fn rev_try_schedule_scope<R>(
        &mut self,
        label: impl ScheduleLabel,
        f: impl FnOnce(&mut World, &mut RevSchedule) -> R,
    ) -> Result<R, ScheduleMissing> {
        let label = label.intern();
        let mut schedules = self
            .get_resource_mut::<Schedules>()
            .ok_or(ScheduleMissing)?;
        let forward = schedules.remove(ForwardSchedule(label));
        let backward = schedules.remove(BackwardSchedule(label));
        let (Some(forward), Some(backward)) = (forward, backward) else {
            return Err(ScheduleMissing);
        };
        let mut rev_schedule = RevSchedule { forward, backward };
        let r = f(self, &mut rev_schedule);
        let mut schedules = self.resource_mut::<Schedules>();
        schedules.insert(rev_schedule.forward);
        schedules.insert(rev_schedule.backward);
        Ok(r)
    }
}
