use std::fmt::Debug;

use bevy::{
    ecs::{
        change_detection::Res,
        component::Tick,
        entity::{Entity, EntityHashMap},
        system::{ReadOnlySystemParam, SystemMeta, SystemName, SystemParam},
        world::{unsafe_world_cell::UnsafeWorldCell, World},
    },
    utils::default,
};

use crate::{
    error_per_flag,
    log::InitNoneLog,
    meta::{RevDirection, RevMeta},
};

use super::{PackedRevFrame, RevFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastRunError {
    FrameMismatch { expected: Option<RevFrame> },
    OutOfLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMismatch {
    expected: Option<RevFrame>,
}

impl From<FrameMismatch> for LastRunError {
    fn from(FrameMismatch { expected }: FrameMismatch) -> Self {
        Self::FrameMismatch { expected }
    }
}

pub struct RevLastRun<'s> {
    system_last_run: Result<Option<RevFrame>, LastRunError>,
    system_last_run_read: bool,
    system_name: SystemName<'s>,
    error_flag: &'s mut bool,
    entity_last_run_logs: &'s mut EntityHashMap<InitNoneLog<PackedRevFrame>>,
    entity_last_run_allocations: &'s mut Vec<InitNoneLog<PackedRevFrame>>,
    entity_updates: &'s mut EntityHashMap<Result<Option<RevFrame>, LastRunError>>,
    now: RevFrame,
    now_packed: PackedRevFrame,
}

pub struct RevLastRunState {
    meta_state: <Res<'static, RevMeta> as SystemParam>::State,
    name_state: <SystemName<'static> as SystemParam>::State,
    error_flag: bool,
    system_last_run_log: InitNoneLog<RevFrame>,
    entity_last_run_logs: EntityHashMap<InitNoneLog<PackedRevFrame>>,
    entity_last_run_allocations: Vec<InitNoneLog<PackedRevFrame>>,
    entity_updates: EntityHashMap<Result<Option<RevFrame>, LastRunError>>,
}

/// If [`system_last_run`](RevLastRun::system_last_run) is not called and that method would have returned an `Err`,
/// this `Drop` implementation will log the error instead, though only for the first time this happens for this instance.
impl<'s> Drop for RevLastRun<'s> {
    fn drop(&mut self) {
        if !self.system_last_run_read && self.system_last_run.is_err() {
            error_per_flag!(self.error_flag, "todo {}", self.system_name)
        }
    }
}

impl<'s> RevLastRun<'s> {
    pub fn system_last_run(&mut self) -> Result<Option<RevFrame>, LastRunError> {
        self.system_last_run_read = true;
        self.system_last_run
    }
    pub fn forward(&mut self, entity: Entity) -> Option<RevFrame> {
        if let Some(result) = self.entity_updates.get(&entity) {
            return result.expect("`LastRun::forward` should not be called along `LastRun::*_log` methods in the same system run");
        }
        let log = self
            .entity_last_run_logs
            .entry(entity)
            .or_insert_with(|| self.entity_last_run_allocations.pop().unwrap_or_default());
        let last_run = log.get().cloned().map(Into::into);
        self.entity_updates.insert(entity, Ok(last_run));
        log.push_present(self.now_packed);
        last_run
    }
    pub fn forward_log(&mut self, entity: Entity) -> Result<Option<RevFrame>, FrameMismatch> {
        if let Some(result) = self.entity_updates.get(&entity) {
            return result.map_err(|err| match err {
                LastRunError::FrameMismatch { expected } => FrameMismatch { expected },
                LastRunError::OutOfLog => panic!("`LastRun::forward_log` should not be called along `LastRun::backward_log` in the same system run")
            });
        }
        let log = self
            .entity_last_run_logs
            .entry(entity)
            .or_insert_with(|| self.entity_last_run_allocations.pop().unwrap_or_default());
        let last_run = log.get().cloned().map(Into::into);
        let expected = match log.forward_log() {
            Ok(expected) => *expected,
            Err(_) => return self.frame_mismatch(entity, None),
        };
        if expected != self.now_packed {
            return self.frame_mismatch(entity, Some(expected.into()));
        }
        self.entity_updates.insert(entity, Ok(last_run));
        Ok(last_run)
    }
    pub fn backward_log(&mut self, entity: Entity) -> Result<Option<RevFrame>, LastRunError> {
        if let Some(result) = self.entity_updates.get(&entity) {
            return *result;
        }
        let log = self
            .entity_last_run_logs
            .entry(entity)
            .or_insert_with(|| self.entity_last_run_allocations.pop().unwrap_or_default());
        let Some(mut expected) = log.get().cloned().map(Into::<RevFrame>::into) else {
            return self.frame_mismatch(entity, None);
        };
        expected = expected.wrapping_sub(1);
        if expected != self.now {
            return self.frame_mismatch(entity, Some(expected));
        }
        if log.backward_log().is_err() {
            return self.out_of_log(entity);
        }
        Ok(log.get().cloned().map(Into::into))
    }
    fn out_of_log(&mut self, entity: Entity) -> Result<Option<RevFrame>, LastRunError> {
        let err = Err(LastRunError::OutOfLog);
        self.entity_updates.insert(entity, err);
        err
    }
    fn frame_mismatch<E: From<FrameMismatch>>(
        &mut self,
        entity: Entity,
        expected: Option<RevFrame>,
    ) -> Result<Option<RevFrame>, E> {
        let err = FrameMismatch { expected };
        self.entity_updates.insert(entity, Err(err.into()));
        Err(err.into())
    }
}

unsafe impl<'s> SystemParam for RevLastRun<'s> {
    type Item<'world, 'state> = RevLastRun<'state>;
    type State = RevLastRunState;

    fn init_state(world: &mut World, system_meta: &mut SystemMeta) -> Self::State {
        let meta_state = <Res<RevMeta> as SystemParam>::init_state(world, system_meta);
        let name_state = <SystemName as SystemParam>::init_state(world, system_meta);
        RevLastRunState {
            meta_state,
            name_state,
            error_flag: false,
            system_last_run_log: default(),
            entity_last_run_logs: default(),
            entity_last_run_allocations: default(),
            entity_updates: default(),
        }
    }

    unsafe fn validate_param(
        state: &Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell,
    ) -> bool {
        <Res<RevMeta> as SystemParam>::validate_param(&state.meta_state, system_meta, world)
    }

    unsafe fn get_param<'world, 'state>(
        state: &'state mut Self::State,
        system_meta: &SystemMeta,
        world: UnsafeWorldCell<'world>,
        change_tick: Tick,
    ) -> Self::Item<'world, 'state> {
        // get meta
        let meta = <Res<RevMeta> as SystemParam>::get_param(
            &mut state.meta_state,
            system_meta,
            world,
            change_tick,
        );
        let now = meta.present_world_state();

        // get name
        let system_name = <SystemName as SystemParam>::get_param(
            &mut state.name_state,
            system_meta,
            world,
            change_tick,
        );

        // update system log
        let log = &mut state.system_last_run_log;
        let system_last_run = match meta.get_direction() {
            None => panic!("todo"),
            Some(RevDirection::NotLog) => {
                // shorten entity logs or clear orphaned for reusage
                let allocations = state
                    .entity_last_run_logs
                    .extract_if(|&entity, log| {
                        if world.entities().contains(entity) {
                            log.pop_past_by_logged_at(&meta);
                            false
                        } else {
                            log.clear();
                            true
                        }
                    })
                    .map(|(_, log)| log);
                state.entity_last_run_allocations.extend(allocations);

                // continue updating system log
                log.pop_past_by_logged_at(&meta);
                let last_run = log.get().cloned();
                log.push_present(now);
                Ok(last_run)
            }
            Some(RevDirection::ForwardLog) => {
                let last_run = log.get().cloned();
                match log.forward_log() {
                    Ok(&expected) if expected != now => Err(LastRunError::FrameMismatch {
                        expected: Some(expected),
                    }),
                    Ok(_) => Ok(last_run),
                    Err(_) => Err(LastRunError::FrameMismatch { expected: None }),
                }
            }
            Some(RevDirection::BackwardLog) => {
                if let Some(mut expected) = log.get().cloned() {
                    expected = expected.wrapping_sub(1);
                    if expected != now {
                        Err(LastRunError::FrameMismatch {
                            expected: Some(expected),
                        })
                    } else if log.backward_log().is_err() {
                        Err(LastRunError::OutOfLog)
                    } else {
                        Ok(log.get().cloned())
                    }
                } else {
                    Err(LastRunError::FrameMismatch { expected: None })
                }
            }
        };

        // clear buffer for this run's results
        state.entity_updates.clear();

        RevLastRun {
            system_last_run,
            system_last_run_read: false,
            system_name,
            error_flag: &mut state.error_flag,
            entity_last_run_logs: &mut state.entity_last_run_logs,
            entity_last_run_allocations: &mut state.entity_last_run_allocations,
            entity_updates: &mut state.entity_updates,
            now,
            now_packed: now.into(),
        }
    }
}

unsafe impl<'s> ReadOnlySystemParam for RevLastRun<'s> {}
