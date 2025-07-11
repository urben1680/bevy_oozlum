use std::any::TypeId;

use bevy::{
    ecs::{
        component::{CheckChangeTicks, ComponentId, Tick},
        query::FilteredAccessSet,
        schedule::{BoxedCondition, InternedSystemSet, SystemCondition},
        system::{
            IntoSystem, ReadOnlySystem, RunSystemError, System, SystemIn,
            SystemParamValidationError, SystemStateFlags,
        },
        world::{DeferredWorld, World, unsafe_world_cell::UnsafeWorldCell},
    },
    utils::{default, prelude::DebugName},
};

use crate::{
    log::{OutOfLog, SparseTransitionLog},
    meta::{RevDirection, RevMeta},
    schedule::error_per_flag,
};

pub(super) fn into_rev_condition<Marker>(
    condition: impl SystemCondition<Marker>,
) -> BoxedCondition {
    let condition = RevCondition {
        condition: IntoSystem::into_system(condition),
        meta_id: default(),
        log: default(),
        out_of_log_err: false,
    };
    Box::new(condition)
}

struct RevCondition<T> {
    condition: T,
    meta_id: Option<ComponentId>,
    log: SparseTransitionLog<()>,

    out_of_log_err: bool,
}

impl<T: ReadOnlySystem<In = (), Out = bool>> System for RevCondition<T> {
    type In = ();
    type Out = bool;
    fn name(&self) -> DebugName {
        self.condition.name()
    }
    fn type_id(&self) -> TypeId {
        self.condition.type_id()
    }
    fn flags(&self) -> SystemStateFlags {
        self.condition.flags()
    }
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet<ComponentId> {
        let mut access = self.condition.initialize(world);
        let meta_id = world.register_resource::<RevMeta>();
        self.meta_id = Some(meta_id);
        access.add_unfiltered_resource_read(meta_id); // cannot fail because `condition` is a read-only system
        access
    }
    unsafe fn validate_param_unsafe(
        &mut self,
        world: UnsafeWorldCell,
    ) -> Result<(), SystemParamValidationError> {
        let ptr = unsafe {
            // SAFETY:
            // - Registered read access to resource
            // - T cannot have write access because T: ReadOnlySystem
            world.get_resource_by_id(self.meta_id.unwrap())
        };
        ptr.map(|ptr| unsafe {
            // SAFETY: todo
            ptr.deref::<RevMeta>()
        })
        .ok_or(SystemParamValidationError::invalid::<Self>(
            RevMeta::EXPECT_IN_WORLD,
        ))?
        .get_running_direction()
        .ok_or(SystemParamValidationError::invalid::<Self>(
            RevMeta::EXPECT_RUNNING,
        ))
        .and_then(|_| unsafe {
            // SAFETY: todo
            self.condition.validate_param_unsafe(world)
        })
    }
    unsafe fn run_unsafe(
        &mut self,
        (): SystemIn<'_, Self>,
        world: UnsafeWorldCell,
    ) -> Result<bool, RunSystemError> {
        let ptr = unsafe {
            // SAFETY:
            // - Registered read access to resource
            // - T cannot have write access because T: ReadOnlySystem
            world.get_resource_by_id(self.meta_id.unwrap())
        };
        let meta = ptr
            .map(|ptr| unsafe {
                // SAFETY: todo
                ptr.deref::<RevMeta>()
            })
            .ok_or(SystemParamValidationError::invalid::<Self>(
                RevMeta::EXPECT_IN_WORLD,
            ))?;
        let direction =
            meta.get_running_direction()
                .ok_or(SystemParamValidationError::invalid::<Self>(
                    RevMeta::EXPECT_RUNNING,
                ))?;

        match direction {
            RevDirection::NOT_LOG => {
                let out = unsafe {
                    // SAFETY: condition is readonly so meta reference is allowed to exist while condition runs
                    // todo: other safety comments
                    self.condition.run_unsafe((), world)
                };
                let transition = match out {
                    Ok(true) => Some(()),
                    _ => None,
                };
                self.log
                    .push_and_pop_past(meta.past_len() as usize, transition);
                out
            }
            // todo: simplify error msg, can only be internal bug
            // todo: upstream systems returning Result<bool, BevyError> be valid conditions
            RevDirection::FORWARD_LOG => match self.log.forward_log() {
                Ok(option) => Ok(option.is_some()),
                Err(OutOfLog) => Ok(error_per_flag!(
                    &mut self.out_of_log_err,
                    "Reversible condition {} got out of log. \
                        Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                        It seems that one or more backward schedule calls were missed. \
                        If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}",
                    self.name()
                )),
            },
            RevDirection::BackwardLog => match self.log.backward_log() {
                Ok(option) => Ok(option.is_some()),
                Err(OutOfLog) => Ok(error_per_flag!(
                    &mut self.out_of_log_err,
                    "Reversible condition {} got out of log. \
                        Make sure the reversible schedule this condition is in is correctly called in both the forward and backward direction. \
                        It seems that one or more forward schedule calls were missed. \
                        If this condition is in the RevUpdate schedule, this is likely a crate bug.\n{meta:?}",
                    self.name()
                )),
            },
        }
    }
    fn apply_deferred(&mut self, _world: &mut World) {}
    fn queue_deferred(&mut self, _world: DeferredWorld) {}
    fn default_system_sets(&self) -> Vec<InternedSystemSet> {
        self.condition.default_system_sets()
    }
    fn check_change_tick(&mut self, change_tick: CheckChangeTicks) {
        self.condition.check_change_tick(change_tick);
    }
    fn get_last_run(&self) -> Tick {
        self.condition.get_last_run()
    }
    fn set_last_run(&mut self, last_run: Tick) {
        self.condition.set_last_run(last_run);
    }
}

// SAFETY:
// todo
unsafe impl<T: ReadOnlySystem<In = (), Out = bool>> ReadOnlySystem for RevCondition<T> {}
