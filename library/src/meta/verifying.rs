use std::{num::NonZeroUsize, ops::Deref};

use bevy::{
    ecs::{
        component::ComponentId,
        system::{ReadOnlySystemParam, Res, SystemMeta, SystemParam},
        world::World,
    },
    log::error,
};

use crate::log::{LoggedAt, OutOfLog, PackedRevFrame, StateLog};

use super::{RevDirection, RevMeta};

/// `RevMeta` system param wrapper to keep track of the system's running frames.
///
/// Can be used to automatically verify that the system is running at the right
/// frame and to get the frame number the system last ran, if any.
#[derive(Debug, Copy, Clone)]
pub struct VerifyingRevMeta<'w, 's> {
    meta: &'w RevMeta,
    last_run_or_err: Result<Option<NonZeroUsize>, VerifyError<'s>>,
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
    /// Returns None if the system did not run in the past.
    ///
    /// Note that this is the chronical last run and therefore is always in the past.
    ///
    /// # Panics
    ///
    /// Panics if the update of the value failed or if the the current frame does not
    /// match with the frame that is logged by this SystemParam.
    ///
    /// [`Self::get_last_run`] is a fallible variant.
    pub fn last_run(&self) -> Option<usize> {
        self.get_last_run().unwrap_or_else(|err| panic!(
            "VerifyingRevMeta::last_run panicked: VerifyingRevMeta::get_param failed previously, see log\n{err:#?}"
        ))
    }

    /// Get the frame the system last ran.
    ///
    /// Returns None if the system did not run in the past.
    ///
    /// Note that this is the chronical last run and therefore is always in the past.
    ///
    /// Returns Err if the update of the value failed or if the the current frame does not
    /// match with the frame that is logged by this SystemParam.
    pub fn get_last_run(&self) -> Result<Option<usize>, VerifyError> {
        self.last_run_or_err
            .map(|last_run| last_run.map(NonZeroUsize::get))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct VerifyError<'s> {
    pub frame_log_at_err: &'s StateLog<PackedRevFrame>,
    pub meta_at_err: &'s RevMeta,
}

pub struct VerifyingRevMetaState {
    meta: ComponentId,
    frame_log: StateLog<PackedRevFrame>,
    meta_at_err: Option<RevMeta>,
}

impl VerifyingRevMetaState {
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
    fn update_state_get_last_run(
        &mut self,
        meta: &RevMeta,
        system_name: &str,
    ) -> Option<NonZeroUsize> {
        let mut last_run = 0;
        match meta.get_direction() {
            Some(RevDirection::NotLog) => {
                last_run = self.frame_log.logged_at();
                self.frame_log
                    .pop_past_by_logged_at(meta.first_world_state());
                self.frame_log.push_present(meta.present_world_state.into());
            }
            Some(RevDirection::ForwardLog) => {
                last_run = self.frame_log.logged_at();
                if self.frame_log.forward_log() == Err(OutOfLog) {
                    self.out_of_log("forward", meta, system_name);
                } else if self.frame_log.logged_at() != meta.present_world_state() {
                    self.mismatch("forward", meta, system_name);
                }
            }
            Some(RevDirection::BackwardLog) => {
                if self.frame_log.logged_at() - 1 != meta.present_world_state() {
                    self.mismatch("backward", meta, system_name);
                } else if self.frame_log.backward_log() == Err(OutOfLog) {
                    self.out_of_log("backward", meta, system_name);
                }
                last_run = self.frame_log.logged_at();
            }
            None => self.non_rev_schedule(meta, system_name),
        };
        NonZeroUsize::new(last_run)
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
    fn mismatch(&mut self, direction: &str, meta: &RevMeta, system_name: &str) {
        let mut expected = self.frame_log.logged_at();
        if direction == "backward" {
            expected -= 1;
        }
        let actual = meta.present_world_state();
        error!(
            "VerifyingRevMeta::get_param failed: system \"{system_name}\" is expected to run at frame {expected} \
            but ran at  frame {actual} during {direction} log schedule{}\n{meta:#?}\n{:#?}",
            Self::SUGGESTION, self.frame_log
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
        let meta = Res::<RevMeta>::init_state(world, system_meta);

        // 0 is a special value here, during forward schedules the current frame is never 0, so if the
        // value passed to Self::Item is 0, this indicates that the system did not run in a past frame.
        // This works better than wrapping the log in an Option that becomes Some at the first run as
        // then undoing that call would be undistinguishable to an out-of-log error.
        todo!("let logged_at = GetLoggedAt::from(0);"); // 0 nicht mehr besonders bei wrapping logik

        VerifyingRevMetaState {
            meta,
            frame_log: todo!(),
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
            .get_resource_by_id(state.meta)
            .expect("todo, upcoming verify params feature")
            .deref(); //SAFETY: correct ComponentId from Res::<RevMeta>::init_state
        state.get_param(meta, system_meta.name())
    }
}

// SAFETY: Only reads RevMeta
unsafe impl ReadOnlySystemParam for VerifyingRevMeta<'_, '_> {}
