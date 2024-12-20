use std::ops::Deref;

use bevy::{
    ecs::{
        component::ComponentId,
        system::{ReadOnlySystemParam, Res, SystemMeta, SystemParam},
        world::World,
    },
    log::error,
    utils::default,
};

use crate::{
    log::{InitNoneLog, OutOfLog},
    PackedRevFrame, RevFrame,
};

use super::{RevDirection, RevMeta};

/// `Res<RevMeta>` wrapper to keep track of the system's running frames.
///
/// Can be used to automatically verify that the system is running at the right
/// frame and to get the frame number the system last ran, if any.
#[derive(Debug, Copy, Clone)]
pub struct VerifyingRevMeta<'w, 's> {
    meta: &'w RevMeta,
    pub last_run_or_err: Result<Option<RevFrame>, VerifyError<'s>>,
}

impl Deref for VerifyingRevMeta<'_, '_> {
    type Target = RevMeta;
    fn deref(&self) -> &Self::Target {
        self.meta
    }
}

impl VerifyingRevMeta<'_, '_> {
    /// Get the frame the system last ran.
    ///
    /// Returns None if the system did not run in the past or if the last run is no longer in log.
    ///
    /// Note that this is the chronical last run and therefore is always in the past.
    ///
    /// # Panics
    ///
    /// Panics if the update of the value failed or if the the current frame does not
    /// match with the frame that is logged by this SystemParam.
    ///
    /// Access [`Self::last_run_or_err`] if panic is undesired.
    pub fn last_run(&self) -> Option<RevFrame> {
        self.last_run_or_err.unwrap_or_else(|err| panic!(
            "VerifyingRevMeta::last_run panicked: VerifyingRevMeta::get_param failed previously, see log\n{err:#?}"
        ))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VerifyError<'s> {
    pub frame_log_at_err: &'s InitNoneLog<PackedRevFrame>,
    pub meta_at_err: &'s RevMeta,
}

pub struct VerifyingRevMetaState {
    meta_id: ComponentId,
    frame_log: InitNoneLog<PackedRevFrame>,
    meta_at_err: Option<RevMeta>,
}

impl VerifyingRevMetaState {
    fn get(&self) -> Option<RevFrame> {
        self.frame_log.get().cloned().map(Into::into)
    }
    fn get_param<'w, 's>(
        &'s mut self,
        meta: &'w RevMeta,
        system_name: &str,
    ) -> VerifyingRevMeta<'w, 's> {
        let mut last_run = None;
        if self.meta_at_err.is_none() {
            last_run = self.update_state_get_last_run(meta, system_name);
        }
        match self.meta_at_err.as_ref() {
            None => VerifyingRevMeta {
                meta,
                last_run_or_err: Ok(last_run),
            },
            Some(meta_at_err) => VerifyingRevMeta {
                meta,
                last_run_or_err: Err(VerifyError {
                    frame_log_at_err: &self.frame_log,
                    meta_at_err,
                }),
            },
        }
    }
    fn update_state_get_last_run(&mut self, meta: &RevMeta, system_name: &str) -> Option<RevFrame> {
        let mut last_run;
        match meta.get_direction() {
            Some(RevDirection::NotLog) => {
                last_run = self.get();
                self.frame_log
                    .push_present(meta.present_world_state().into());
                self.frame_log.pop_past_by_logged_at(meta);
                if self.frame_log.states_is_empty() {
                    last_run = None;
                }
            }
            Some(RevDirection::ForwardLog) => {
                last_run = self.get();
                match self.frame_log.forward_log() {
                    Ok(()) => {
                        let this_run = self.frame_log.get().unwrap().clone().into();
                        if this_run != meta.present_world_state() {
                            self.mismatch("forward", meta, this_run, system_name);
                        }
                    }
                    Err(OutOfLog) => self.out_of_log("forward", meta, system_name),
                }
            }
            Some(RevDirection::BackwardLog) => {
                match self.get() {
                    Some(mut this_run) => {
                        this_run = this_run.wrapping_sub(1);
                        if this_run != meta.present_world_state() {
                            self.mismatch("backward", meta, this_run, system_name);
                        } else {
                            let _ok = self.frame_log.backward_log();
                        }
                    }
                    None => self.out_of_log("backward", meta, system_name),
                }
                last_run = self.get();
            }
            None => {
                self.non_rev_schedule(meta, system_name);
                last_run = None;
            }
        };
        last_run
    }
    const SUGGESTION: &'static str = ", check if the schedule this system is added to is actually a reversible \
        schedule by using `rev_` prefixed methods on the `App` and that the schedule and is correctly triggered";
    fn out_of_log(&mut self, direction: &str, meta: &RevMeta, system_name: &str) {
        error!(
            "VerifyingRevMeta::get_param failed: system \"{system_name}\" is out of log during {direction} log \
            schedule, at least once a run during another schedule was missed{}\n{meta:#?}\n{:#?}",
            Self::SUGGESTION, self.frame_log
        );
        self.meta_at_err = Some(meta.clone());
    }
    fn mismatch(&mut self, direction: &str, meta: &RevMeta, expected: RevFrame, system_name: &str) {
        error!(
            "VerifyingRevMeta::get_param failed: system \"{system_name}\" is expected to run at frame {expected:?} \
            but ran at  frame {:?} during {direction} log schedule{}\n{meta:#?}\n{:#?}",
            meta.present_world_state(), Self::SUGGESTION, self.frame_log
        );
        self.meta_at_err = Some(meta.clone());
    }
    fn non_rev_schedule(&mut self, meta: &RevMeta, system_name: &str) {
        error!(
            "VerifyingRevMeta::get_param failed: run of system \"{system_name}\" happened during non-reversible \
            schedule{}\n{meta:#?}\n{:#?}",
            Self::SUGGESTION, self.frame_log
        );
        self.meta_at_err = Some(meta.clone());
    }
}

unsafe impl SystemParam for VerifyingRevMeta<'_, '_> {
    type Item<'world, 'state> = VerifyingRevMeta<'world, 'state>;
    type State = VerifyingRevMetaState;
    fn init_state(world: &mut World, system_meta: &mut SystemMeta) -> Self::State {
        VerifyingRevMetaState {
            meta_id: Res::<RevMeta>::init_state(world, system_meta),
            frame_log: default(),
            meta_at_err: None,
        }
    }
    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        system_meta: &SystemMeta,
        world: bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell<'world>,
        _change_tick: bevy::ecs::component::Tick,
    ) -> Self::Item<'world, 'state> {
        let meta: &RevMeta = world
            .get_resource_by_id(state.meta_id)
            .expect("validate_param returned true for this method to run")
            .deref(); //SAFETY: correct ComponentId from Res::<RevMeta>::init_state
        state.get_param(meta, system_meta.name())
    }
    unsafe fn validate_param(
        state: &Self::State,
        _system_meta: &SystemMeta,
        world: bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell,
    ) -> bool {
        world.get_resource_by_id(state.meta_id).is_some()
    }
}

// SAFETY: Only reads RevMeta
unsafe impl ReadOnlySystemParam for VerifyingRevMeta<'_, '_> {}

#[cfg(test)]
mod test {
    use bevy::ecs::{
        schedule::{Schedule, Schedules},
        system::Resource,
    };

    use crate::RevUpdate;

    use super::*;

    #[derive(Resource)]
    struct ShouldErr;

    fn system(verify: VerifyingRevMeta, should_err: Option<Res<ShouldErr>>) {
        match should_err {
            None => assert!(verify.last_run_or_err.is_ok()),
            Some(_) => assert!(verify.last_run_or_err.is_err()),
        }
    }

    fn setup() -> World {
        let mut schedule = Schedule::new(RevUpdate);
        schedule.add_systems(system);

        let mut schedules = Schedules::new();
        schedules.insert(schedule);

        let mut world = World::new();
        world.insert_resource(RevMeta::new(None, None, false));
        world.insert_resource(schedules);
        world
    }

    fn log_to(world: &mut World, to: u32) {
        assert!(world
            .resource_mut::<RevMeta>()
            .queue_log(RevFrame::checked_new(to))
            .is_ok());
    }

    fn skip_rev_schedule(world: &mut World) {
        world.resource_mut::<RevMeta>().update(|_, _| ());
        world.insert_resource(ShouldErr);
    }

    #[test]
    fn no_panic() {
        let mut world = setup();

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 1
        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 2

        log_to(&mut world, 0);

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 1
        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 0

        log_to(&mut world, 2);

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 1
        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 2
    }

    #[test]
    fn panic_backward_missed() {
        let mut world = setup();

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 1
        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 2

        log_to(&mut world, 0);
        skip_rev_schedule(&mut world);

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 0
    }

    #[test]
    fn panic_forward_missed() {
        let mut world = setup();

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 1
        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 2

        log_to(&mut world, 0);

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 1
        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 0

        log_to(&mut world, 2);
        skip_rev_schedule(&mut world);

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 2
    }

    #[test]
    fn panic_out_of_log() {
        let mut world = setup();
        skip_rev_schedule(&mut world);
        log_to(&mut world, 0);

        assert_eq!(RevMeta::try_update_world(&mut world), Ok(())); // now: 0
    }
}
