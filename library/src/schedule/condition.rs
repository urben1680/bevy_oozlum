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

/// Wrap a `condition` into a reversivle `BoxedCondition` that logs the system output at
/// [`RevDirection::NOT_LOG`] and traverses the log during [log directions](RevDirection::is_log),
/// bypassing the inner system.
pub(super) fn into_rev_condition<Marker>(
    condition: impl SystemCondition<Marker>,
) -> BoxedCondition {
    let condition = RevCondition {
        condition: IntoSystem::into_system(condition),
        meta_id: Default::default(),
        ok_true: Default::default(),
        err_failed: Default::default(),
        failed: Default::default(),
    };
    Box::new(condition)
}

/// Condition wrapper to turn it into a reversible condition.
struct RevCondition<T> {
    /// Wrapped condition passed to
    /// [`IntoRevScheduleConfigs::rev_run_if`](super::IntoRevScheduleConfigs::rev_run_if).
    condition: T,

    /// [`ComponentId`] of [`RevMeta`].
    meta_id: Option<ComponentId>,

    ok_true: UpdateLog,

    err_failed: UpdateLog,

    failed: TransitionLog<String>,
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
        access.add_unfiltered_resource_read(meta_id);
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

        self.ok_true.pre_update(meta);
        self.err_failed.pre_update(meta);
        self.failed.pre_update(meta);

        match direction {
            RevDirection::NOT_LOG => {
                let result = unsafe {
                    // SAFETY: condition is readonly so meta reference is allowed to exist while condition runs
                    // todo: other safety comments
                    self.condition.run_unsafe((), world)
                };

                match result {
                    Ok(false) | Err(RunSystemError::Skipped(_)) => Ok(false),
                    Ok(true) => {
                        self.ok_true.push_get_past_len(meta);
                        Ok(true)
                    }
                    Err(RunSystemError::Failed(failed)) => {
                        let past_len = self.err_failed.push_get_past_len(meta);
                        self.failed.push(past_len, format!("{failed}"));
                        Err(RunSystemError::Failed(failed))
                    }
                }
            }
            // todo: simplify error msg, can only be internal bug
            // todo: upstream systems returning Result<bool, BevyError> be valid conditions
            RevDirection::FORWARD_LOG => {
                if self.ok_true.forward_log(meta) {
                    return Ok(true);
                }
                if self.err_failed.forward_log(meta) {
                    let failed = self.failed.forward_log().unwrap();
                    return Err(RunSystemError::Failed(failed.as_str().into()));
                }
                Ok(false)
            }
            RevDirection::BackwardLog => {
                if self.ok_true.backward_log(meta) {
                    return Ok(true);
                }
                if self.err_failed.backward_log(meta) {
                    let failed = self.failed.backward_log().unwrap();
                    return Err(RunSystemError::Failed(failed.as_str().into()));
                }
                Ok(false)
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
