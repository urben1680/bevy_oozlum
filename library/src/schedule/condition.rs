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
        ok_true_log: Default::default(),
        err_failed_log: Default::default(),
        failed_log: Default::default(),
    };
    Box::new(condition)
}

/// Condition wrapper to turn it into a reversible condition.
struct RevCondition<T> {
    /// Wrapped condition that was passed to
    /// [`IntoRevScheduleConfigs::rev_run_if`](super::IntoRevScheduleConfigs::rev_run_if).
    condition: T,

    /// [`ComponentId`] of [`RevMeta`].
    meta_id: Option<ComponentId>,

    /// A `bool`-like log for frames in which [`Self::condition`] returned `Ok(true)`.
    ok_true_log: UpdateLog,

    /// A `bool`-like log for frames in which [`Self::condition`] returned
    /// [`Err(RunSystemError::Failed(_))`](RunSystemError::Failed).
    err_failed_log: UpdateLog,

    /// A log that contains [`BevyError`](bevy_ecs::error::BevyError) from [`Self::condition`] as
    /// `String`s.
    failed_log: TransitionLog<String>,
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
        // RevMeta validation happens in run_unsafe as its failing verification is never a skipping
        // error but an error that must be handled
        unsafe {
            // SAFETY: in `initialize` T returned the required access to make this safe
            self.condition.validate_param_unsafe(world)
        }
    }
    unsafe fn run_unsafe(
        &mut self,
        (): SystemIn<'_, Self>,
        world: UnsafeWorldCell,
    ) -> Result<bool, RunSystemError> {
        let ptr = unsafe {
            // SAFETY:
            // - Registered read access to resource in `initialize`
            // - T cannot have write access on the resource because T: ReadOnlySystem
            world.get_resource_by_id(self.meta_id.unwrap())
        };
        let meta = ptr
            .map(|ptr| unsafe {
                // SAFETY: `Self::meta_id` is the id of `RevMeta`, so Ptr is its erased pointee type
                ptr.deref::<RevMeta>()
            })
            .ok_or_else(|| RunSystemError::Failed("`RevMeta` is missing but required".into()))?;
        let direction = meta.get_running_direction().ok_or_else(|| {
            RunSystemError::Failed(
                "`RevMeta::get_running_direction` did not return the required `RevDirection".into(),
            )
        })?;

        self.ok_true_log.pre_update(meta);
        self.err_failed_log.pre_update(meta);
        self.failed_log.pre_update(meta);

        match direction {
            RevDirection::NOT_LOG => {
                let result = unsafe {
                    // SAFETY:
                    // - in `initialize` T returned the required access to make this safe
                    // - T is readonly so meta reference is allowed to exist while T runs
                    self.condition.run_unsafe((), world)
                };

                match result {
                    Ok(false) | Err(RunSystemError::Skipped(_)) => Ok(false),
                    Ok(true) => {
                        self.ok_true_log.push_get_past_len(meta);
                        Ok(true)
                    }
                    Err(RunSystemError::Failed(failed)) => {
                        let past_len = self.err_failed_log.push_get_past_len(meta);
                        self.failed_log.push(past_len, failed.to_string());
                        Err(RunSystemError::Failed(failed))
                    }
                }
            }
            RevDirection::FORWARD_LOG => {
                if self.ok_true_log.forward_log(meta) {
                    return Ok(true);
                }
                if self.err_failed_log.forward_log(meta) {
                    let failed = self.failed_log.forward_log().unwrap();
                    return Err(RunSystemError::Failed(failed.as_str().into()));
                }
                Ok(false)
            }
            RevDirection::BackwardLog => {
                if self.ok_true_log.backward_log(meta) {
                    return Ok(true);
                }
                if self.err_failed_log.backward_log(meta) {
                    let failed = self.failed_log.backward_log().unwrap();
                    return Err(RunSystemError::Failed(failed.as_str().into()));
                }
                Ok(false)
            }
        }
    }
    fn apply_deferred(&mut self, _world: &mut World) {
        unreachable!("reversible conditions do not get their deferred parameters applied")
    }
    fn queue_deferred(&mut self, _world: DeferredWorld) {
        unreachable!("reversible conditions are not used as observers")
    }
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
// - T is `ReadOnlySystem`
// - RevCondition additionally only reads `RevMeta` resource
unsafe impl<T: ReadOnlySystem<In = (), Out = bool>> ReadOnlySystem for RevCondition<T> {}
