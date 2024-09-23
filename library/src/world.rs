use bevy::
    ecs::{schedule::{ScheduleLabel, Schedules}, world::World}
;

use crate::{
    app::RevSchedule,
    meta::{Direction, RevMeta},
    BackwardSchedule, ForwardSchedule,
};

#[derive(Clone, Debug)]
pub enum TryRunRevError {
    ScheduleMissing,
    RevMetaMissing,
    NoRevScheduleRunning(RevMeta),
}

#[derive(Clone, Copy, Debug)]
pub struct ScheduleMissing;

pub trait RevWorld {
    fn rev_run_schedule(&mut self, label: impl ScheduleLabel);
    fn rev_try_run_schedule(&mut self, label: impl ScheduleLabel) -> Result<(), TryRunRevError>;
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
    fn rev_try_run_schedule(&mut self, label: impl ScheduleLabel) -> Result<(), TryRunRevError> {
        let meta = self
            .get_resource::<RevMeta>()
            .ok_or(TryRunRevError::RevMetaMissing)?;
        match meta
            .get_direction()
            .ok_or_else(|| TryRunRevError::NoRevScheduleRunning(meta.clone()))?
        {
            Direction::Forward { .. } => self.rev_try_run_forward_schedule(label),
            Direction::BackwardLog => self.rev_try_run_backward_schedule(label),
        }
        .map_err(|ScheduleMissing| TryRunRevError::ScheduleMissing)
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
