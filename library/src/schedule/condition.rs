use crate::{
    log::{TransitionLog, UpdateLog},
    meta::{RevDirection, RevMeta},
};
use bevy_ecs::{
    component::{CheckChangeTicks, ComponentId, Tick},
    query::FilteredAccessSet,
    schedule::{BoxedCondition, InternedSystemSet, SystemCondition},
    system::{
        IntoSystem, ReadOnlySystem, RunSystemError, System, SystemIn, SystemParamValidationError,
        SystemStateFlags,
    },
    world::{DeferredWorld, World, unsafe_world_cell::UnsafeWorldCell},
};
use bevy_utils::prelude::DebugName;
use core::any::TypeId;

pub(super) fn into_rev_condition<Marker>(
    condition: impl SystemCondition<Marker>,
) -> BoxedCondition {
    let condition = RevCondition {
        condition: IntoSystem::into_system(condition),
        meta_id: Default::default(),
        run_log: Default::default(),
        run_or_err_log: Default::default(),
    };
    Box::new(condition)
}

struct RevCondition<T> {
    condition: T,
    meta_id: Option<ComponentId>,
    run_log: UpdateLog,
    run_or_err_log: TransitionLog<Result<(), Box<RevRunSystemError>>>,
}

#[derive(Clone)]
enum RevRunSystemError {
    Skipped(SystemParamValidationError),
    Failed(String),
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
    fn initialize(&mut self, world: &mut World) -> FilteredAccessSet {
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

        self.run_log.pre_update(meta);
        self.run_or_err_log.pre_update(meta);

        match direction {
            RevDirection::NOT_LOG => {
                let result = unsafe {
                    // SAFETY: condition is readonly so meta reference is allowed to exist while condition runs
                    // todo: other safety comments
                    self.condition.run_unsafe((), world)
                };

                match result {
                    Ok(false) => Ok(false),
                    Ok(true) => {
                        let past_len = self.run_log.push_get_past_len(meta);
                        self.run_or_err_log.push(past_len, Ok(()));
                        Ok(true)
                    }
                    Err(RunSystemError::Skipped(skipped)) => {
                        let past_len = self.run_log.push_get_past_len(meta);
                        self.run_or_err_log.push(
                            past_len,
                            Err(Box::new(RevRunSystemError::Skipped(skipped.clone()))),
                        );
                        Err(RunSystemError::Skipped(skipped))
                    }
                    Err(RunSystemError::Failed(failed)) => {
                        let past_len = self.run_log.push_get_past_len(meta);
                        self.run_or_err_log.push(
                            past_len,
                            Err(Box::new(RevRunSystemError::Failed(format!("{failed}")))),
                        );
                        Err(RunSystemError::Failed(failed))
                    }
                }
            }
            // todo: simplify error msg, can only be internal bug
            // todo: upstream systems returning Result<bool, BevyError> be valid conditions
            RevDirection::FORWARD_LOG => {
                if self.run_log.forward_log(meta) {
                    match self.run_or_err_log.forward_log() {
                        Ok(Ok(_)) => Ok(true),
                        Ok(Err(err)) => match &**err {
                            RevRunSystemError::Skipped(skipped) => {
                                Err(RunSystemError::Skipped(skipped.clone()))
                            }
                            RevRunSystemError::Failed(failed) => {
                                Err(RunSystemError::Failed(failed.as_str().into()))
                            }
                        },
                        Err(out_of_log) => panic!("todo"),
                    }
                } else {
                    Ok(false)
                }
            }
            RevDirection::BackwardLog => {
                if self.run_log.backward_log(meta) {
                    match self.run_or_err_log.backward_log() {
                        Ok(Ok(_)) => Ok(true),
                        Ok(Err(err)) => match &**err {
                            RevRunSystemError::Skipped(skipped) => {
                                Err(RunSystemError::Skipped(skipped.clone()))
                            }
                            RevRunSystemError::Failed(failed) => {
                                Err(RunSystemError::Failed(failed.as_str().into()))
                            }
                        },
                        Err(out_of_log) => panic!("todo"),
                    }
                } else {
                    Ok(false)
                }
            }
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
